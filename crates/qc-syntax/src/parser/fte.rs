// SPDX-License-Identifier: GPL-3.0-or-later

//! The full FTEQCC dialect's declarations, types and terms (`docs/spec/dialect-fteqcc.md`).
//!
//! FTEQCC's `QCC_PR_ParseType` knows every type name when it parses, so it can tell a type
//! from a value; this parser runs before names are known, so it decides by shape. At file level
//! every declaration starts with qualifiers and a type, and any name that is not a statement
//! word may be a type (FTEF-15.1). In a body a statement is a declaration when it starts with a
//! qualifier, a type word, or a name followed by a name, `*` and a name, or a parameter list and
//! a name (FTEF-16.1). Each rule is the parse-time shape of what FTEQCC decides by type, so no
//! program FTEQCC accepts is read another way.

use qc_lexer::{Code, LexFlag, Span};
use qc_preproc::{Enabled, FteFlag, Keyword};

use super::{Member, Parser, Site, TypeShape, UNARY_PRIORITY};
use crate::c_scalar::{CContext, CScalar, CWord, CWords, REGISTERED_TYPES};
use crate::kind::SyntaxKind as K;

/// Most parameters an FTEQCC function type may have: `MAX_PARMS+MAX_EXTRA_PARMS` in
/// `QCC_PR_ParseFunctionType` ("Too many parameters. Sorry. (limit is %i)"), 8 and 128
/// (FTEF-15.4).
pub(super) const FTE_MAX_PARMS: usize = 136;

/// The qualifiers `QCC_PR_ParseDefs` reads before a type, each with the keyword
/// `QCC_PR_CheckKeyword` is given for it (FTEF-15.1, FTEF-20.7); `typedef`, which the loop reads
/// too, starts a typedef of its own. `deprecated` is given `false`, so only `__deprecated` is one.
const QUALIFIERS: [(&str, Enabled); 20] = [
    ("typedef", Enabled::By(Keyword::Typedef)),
    ("extern", Enabled::By(Keyword::Extern)),
    ("auto", Enabled::By(Keyword::Auto)),
    ("register", Enabled::By(Keyword::Register)),
    ("static", Enabled::By(Keyword::Static)),
    ("shared", Enabled::By(Keyword::Shared)),
    ("const", Enabled::By(Keyword::Const)),
    ("var", Enabled::By(Keyword::Var)),
    ("nonstatic", Enabled::By(Keyword::Nonstatic)),
    ("unused", Enabled::By(Keyword::Unused)),
    ("noref", Enabled::By(Keyword::Noref)),
    ("used", Enabled::By(Keyword::Used)),
    ("nosave", Enabled::By(Keyword::Nosave)),
    ("strip", Enabled::By(Keyword::Strip)),
    ("ignore", Enabled::By(Keyword::Ignore)),
    ("inline", Enabled::By(Keyword::Inline)),
    ("wrap", Enabled::By(Keyword::Wrap)),
    ("weak", Enabled::By(Keyword::Weak)),
    ("accumulate", Enabled::By(Keyword::Accumulate)),
    ("deprecated", Enabled::Never),
];

/// The words a statement of `QCC_PR_ParseStatement` that starts with one defines locals at,
/// each tested with `STRCMP`, exactly, with its keyword on (FTEF-20.14): `_Bool` always.
const LOCAL_WORDS: [(&str, Enabled); 26] = [
    ("var", Enabled::By(Keyword::Var)),
    ("noref", Enabled::By(Keyword::Noref)),
    ("string", Enabled::By(Keyword::String)),
    ("float", Enabled::By(Keyword::Float)),
    ("double", Enabled::By(Keyword::Double)),
    ("entity", Enabled::By(Keyword::Entity)),
    ("vector", Enabled::By(Keyword::Vector)),
    ("integer", Enabled::By(Keyword::Integer)),
    ("unsigned", Enabled::By(Keyword::Unsigned)),
    ("signed", Enabled::By(Keyword::Signed)),
    ("long", Enabled::By(Keyword::Long)),
    ("int", Enabled::By(Keyword::Int)),
    ("short", Enabled::By(Keyword::Short)),
    ("char", Enabled::By(Keyword::Char)),
    ("_Bool", Enabled::Always),
    ("register", Enabled::By(Keyword::Register)),
    ("volatile", Enabled::By(Keyword::Volatile)),
    ("static", Enabled::By(Keyword::Static)),
    ("class", Enabled::By(Keyword::Class)),
    ("struct", Enabled::By(Keyword::Struct)),
    ("union", Enabled::By(Keyword::Union)),
    ("enum", Enabled::By(Keyword::Enum)),
    ("extern", Enabled::By(Keyword::Extern)),
    ("auto", Enabled::By(Keyword::Auto)),
    ("typedef", Enabled::By(Keyword::Typedef)),
    ("const", Enabled::By(Keyword::Const)),
];

/// The class member qualifiers of `QCC_PR_ParseEntClass` (FTEF-9.2, FTEF-9.3), each read with
/// `QCC_PR_CheckKeyword(1, ...)` (FTEF-20.11).
const MEMBER_QUALIFIERS: [&str; 8] = [
    "nonvirtual",
    "virtual",
    "static",
    "const",
    "var",
    "ignore",
    "strip",
    "inline",
];

/// The words that, under an FTEQCC dialect, never name a type the program defines at the token
/// they are, each with its keyword (FTE-14.2): statement and construct keywords.
const RESERVED: [(&str, Enabled); 28] = [
    ("return", Enabled::Always),
    ("while", Enabled::Always),
    ("do", Enabled::Always),
    ("if", Enabled::Always),
    ("else", Enabled::Always),
    ("not", Enabled::Always),
    ("local", Enabled::By(Keyword::Local)),
    ("for", Enabled::By(Keyword::For)),
    ("switch", Enabled::By(Keyword::Switch)),
    ("case", Enabled::By(Keyword::Case)),
    ("default", Enabled::By(Keyword::Default)),
    ("break", Enabled::By(Keyword::Break)),
    ("continue", Enabled::By(Keyword::Continue)),
    ("typedef", Enabled::By(Keyword::Typedef)),
    ("struct", Enabled::By(Keyword::Struct)),
    ("union", Enabled::By(Keyword::Union)),
    ("enum", Enabled::By(Keyword::Enum)),
    ("enumflags", Enabled::By(Keyword::Enumflags)),
    ("class", Enabled::By(Keyword::Class)),
    ("accessor", Enabled::By(Keyword::Accessor)),
    ("goto", Enabled::By(Keyword::Goto)),
    ("loop", Enabled::By(Keyword::Loop)),
    ("until", Enabled::By(Keyword::Until)),
    ("state", Enabled::By(Keyword::State)),
    ("thinktime", Enabled::By(Keyword::Thinktime)),
    ("using", Enabled::By(Keyword::Using)),
    ("asm", Enabled::By(Keyword::Asm)),
    ("exit", Enabled::Never),
];

/// Where `QCC_PR_ParseType` reads a type, which decides its `ignoreptr` (FTEF-21.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// A declaration's type, read by `QCC_PR_ParseDefs` with `ignoreptr` when `qcfuncs` is off.
    Declaration,
    /// A struct member's, read with `QCC_PR_ParseType(false, false, true)`: it takes no `*`
    /// whether or not `qcfuncs` is on (FTEF-6.7).
    Member,
    /// Any other: a parameter's, a cast's, a field's inner type of either.
    Nested,
}

impl Parser<'_> {
    /// Whether a declaration whose qualifiers were read leaves its type out (FTEF-19.21): a
    /// name followed by what ends a declarator, `;`, `,`, `=` or `[`, where
    /// `QCC_PR_ParseType` with `silentfail` finds no type and `QCC_PR_ParseDefs` assumes
    /// `int`.
    pub(super) fn fte_type_omitted(&self) -> bool {
        match self.current().map(|t| t.kind) {
            Some(K::Name) => self
                .nth(1)
                .is_some_and(|t| matches!(t.kind, K::Semicolon | K::Comma | K::Eq | K::LBracket)),
            // `QCC_PR_ParseType` with `silentfail` returns no type for a token that is no name,
            // so `static;` and `const *x;` are an `int`'s definitions (FTEF-19.21), and `var =
            // 1;` one whose name `QCC_PR_ParseName` refuses, "\"=\" - not a name" (FTEF-19.73).
            Some(K::Dot | K::DotDot) | None => false,
            Some(_) => true,
        }
    }

    /// Whether the token `n` ahead is `volatile` as `QCC_PR_ParseType` reads it, with its keyword,
    /// which by default only `__volatile` spells (FTEF-2.11, FTEF-20.8).
    pub(super) fn at_volatile(&self, n: usize) -> bool {
        self.kw(n, Keyword::Volatile, "volatile")
    }

    /// FTEF-2.11: "ignoring unsupported const keyword" at the current token.
    fn warn_const(&mut self) {
        if let Some(span) = self.current().map(|t| t.span) {
            self.warning_at(Code::W0226, "ignoring unsupported const keyword", span);
        }
    }

    /// Whether the token `n` ahead is a declaration qualifier `QCC_PR_ParseDefs` reads before a
    /// type with the keywords in force (FTEF-15.1, FTEF-20.7); `nonstatic` only outside a
    /// function (`!pr_scope && QCC_PR_CheckKeyword(keyword_nonstatic, "nonstatic")`).
    pub(super) fn decl_qualifier(&self, n: usize) -> bool {
        QUALIFIERS.iter().any(|&(word, enabled)| {
            (word != "nonstatic" || self.bodies == 0) && self.checks(n, enabled, word)
        })
    }

    /// Whether a statement that starts with the token `n` ahead defines locals by one of the
    /// words `QCC_PR_ParseStatement` tests exactly with their keyword on (FTEF-20.14).
    fn starts_local_statement(&self, n: usize) -> bool {
        LOCAL_WORDS.iter().any(|&(word, enabled)| match enabled {
            Enabled::By(keyword) => self.strcmp_kw(n, keyword, word),
            Enabled::Always | Enabled::Never => self
                .nth(n)
                .is_some_and(|t| t.kind == K::Name && t.text == word),
        })
    }

    /// What the loop over C type words reads at the token `n` ahead besides the words: the
    /// keywords and `ILP32` in force there (FTEF-20.9).
    pub(super) fn c_context(&self, n: usize) -> CContext {
        let at = self
            .nth(n)
            .map_or(self.input.source.len(), |t| t.span.start);
        CContext {
            keywords: self.keywords_ahead(n),
            ilp32: self.lexed_with(at, LexFlag::Ilp32),
        }
    }

    /// Whether the token `n` ahead starts a C scalar type `QCC_PR_ParseType` reads with its loop
    /// of C type words (FTEF-2.7, FTEF-20.9): a word of [`CWord::of`] with the keywords in force,
    /// but a bare `float`, which `QCC_TypeForName` finds first.
    pub(super) fn c_type_word(&self, n: usize) -> bool {
        self.nth(n).is_some_and(|t| {
            t.kind == K::Name
                && self.type_word(n) != Some(K::FloatKw)
                && CWord::of(&t.text, self.keywords_ahead(n)).is_some()
        })
    }

    /// Whether `::` is the token `n` ahead and touches the token before it, as `QCC_PR_ParseStatement`
    /// tests `strncmp(pr_file_p, "::", 2)` right after a name.
    fn at_scope_resolution(&self, n: usize) -> bool {
        let before = self.nth(n.saturating_sub(1)).map(|t| t.span.end);
        self.nth(n)
            .is_some_and(|t| t.kind == K::ColonColon && Some(t.span.start) == before)
    }

    /// Whether the token `n` ahead is a name FTEQCC registers as a type, which `QCC_TypeForName`
    /// finds (FTEF-19.20, FTEF-19.33): the vanilla type words, the scalars and the other names of
    /// [`REGISTERED_TYPES`], in any case under `caseinsens`.
    pub(super) fn registered_type(&self, n: usize) -> bool {
        let any_case = self.any_case(n);
        self.nth(n).is_some_and(|tok| {
            let named = |name: &str| {
                if any_case {
                    name.eq_ignore_ascii_case(&tok.text)
                } else {
                    name == tok.text
                }
            };
            tok.kind == K::Name
                && (self.type_word(n).is_some()
                    || self.scalar_name(n).is_some()
                    || REGISTERED_TYPES.iter().any(|name| named(name))
                    || self.types.iter().any(|t| named(&t.name)))
        })
    }

    /// The scalar type the token `n` ahead names, a name FTEQCC registers (`QCC_TypeForName`):
    /// in any case under `caseinsens` (FTEF-19.33).
    pub(super) fn scalar_name(&self, n: usize) -> Option<CScalar> {
        let tok = self.nth(n).filter(|t| t.kind == K::Name)?;
        CScalar::named(&tok.text).or_else(|| {
            if self.any_case(n) {
                CScalar::named(&tok.text.to_ascii_lowercase())
            } else {
                None
            }
        })
    }

    /// Whether the token `n` ahead is a class member qualifier (FTEF-20.11).
    fn member_qualifier(&self, n: usize) -> bool {
        MEMBER_QUALIFIERS
            .iter()
            .any(|word| self.checks(n, Enabled::Always, word))
    }

    /// Whether the token `n` ahead is a parameter qualifier of `QCC_PR_ParseFunctionType`
    /// (FTEF-15.3, FTEF-20.10): `optional` with its keyword, and `inout`, `out` and `in` with
    /// `keyword_inout`.
    fn param_qualifier(&self, n: usize) -> bool {
        self.kw(n, Keyword::Optional, "optional")
            || ["inout", "out", "in"]
                .iter()
                .any(|word| self.kw(n, Keyword::Inout, word))
    }

    /// Whether the token `n` ahead is a statement or construct keyword with the keywords in
    /// force, which no type the program defines is named by there (FTE-14.2).
    pub(super) fn reserved(&self, n: usize) -> bool {
        RESERVED
            .iter()
            .any(|&(word, enabled)| self.checks(n, enabled, word))
    }

    /// Whether the token `n` ahead is a name that can start a type: a vanilla or C type word,
    /// or a name that is no keyword there (a type the program defines, FTEF-7.2).
    pub(super) fn fte_type_name(&self, n: usize) -> bool {
        self.nth(n).is_some_and(|t| t.kind == K::Name)
            && !self.reserved(n)
            && !self.decl_qualifier(n)
    }

    /// The qualifiers before a declaration's type (FTEF-15.1), each its own node; a
    /// `__deprecated` takes its optional `("reason")`; and the attribute lists among them
    /// (FTEF-21.9). `typedef` is one, any number of times and in any place among them
    /// (FTEF-21.14); it is a token of its own. Returns whether it read one, `QCC_PR_ParseDefs`'s
    /// `hadmodifier`.
    pub(super) fn fte_qualifiers(&mut self) -> bool {
        let mut read = false;
        let mut accumulate = false;
        loop {
            if !self.decl_qualifier(0) {
                // FTEF-21.9: `else if ((hadmodifier||(flag_attributes && !pr_scope)) &&
                // QCC_PR_CheckToken("["))`.
                let attributes =
                    read || (self.bodies == 0 && self.flag_ahead(0, FteFlag::Attributes));
                if !(attributes && self.at(K::LBracket)) {
                    break;
                }
                self.attribute_list(&mut accumulate);
                read = true;
                continue;
            }
            read = true;
            if self.kw(0, Keyword::Typedef, "typedef") {
                self.bump_as(K::TypedefKw);
                continue;
            }
            accumulate |= self.kw(0, Keyword::Accumulate, "accumulate");
            let text = self.found();
            let deprecated = self.checks(0, Enabled::Never, "deprecated");
            self.start_node(K::Qualifier);
            match text.as_str() {
                "const" => self.bump_as(K::ConstKw),
                "var" => self.bump_as(K::VarKw),
                "nosave" => self.bump_as(K::NosaveKw),
                _ => self.bump(),
            }
            if deprecated && self.at(K::LParen) {
                self.bump();
                while !self.at(K::RParen) && !self.at(K::Semicolon) && self.current().is_some() {
                    self.bump();
                }
                self.expect(K::RParen, ")");
            }
            self.finish_node();
        }
        read
    }

    /// `[[ ... ]]` (FTEF-21.9): `QCC_PR_CheckToken("[")`, then `QCC_PR_Expect("[")`, then
    /// attributes and `,`s up to a `]`, then `QCC_PR_Expect("]")`.
    fn attribute_list(&mut self, accumulate: &mut bool) {
        self.start_node(K::AttributeList);
        self.bump();
        self.expect(K::LBracket, "[");
        while self.current().is_some() {
            if self.at(K::Comma) {
                self.bump();
            } else if self.at(K::RBracket) {
                self.bump();
                break;
            } else {
                self.attribute(accumulate);
            }
        }
        self.expect(K::RBracket, "]");
        self.finish_node();
    }

    /// One attribute (FTEF-21.9): `alias(NAME)` or `alias("NAME")`, `accumulate`, `eraseable`,
    /// `__deprecated` with an optional `("reason")`, each name read with `QCC_PR_CheckName` but
    /// `__deprecated`'s with `QCC_PR_CheckKeyword(false, ...)`; any other token is "Unknown
    /// attribute", and the tokens up to the next `]` or `,` are passed over.
    fn attribute(&mut self, accumulate: &mut bool) {
        self.start_node(K::Attribute);
        if self.names(0, "alias") {
            self.bump();
            self.expect(K::LParen, "(");
            if self.at(K::Name) {
                self.name();
            } else if self
                .current()
                .is_some_and(|t| matches!(t.kind, K::StringLit | K::FloatLit | K::VectorLit))
            {
                self.bump();
            }
            self.expect(K::RParen, ")");
        } else if self.names(0, "accumulate") {
            if *accumulate {
                let span = self.current_span();
                self.warning_at(
                    Code::W0228,
                    "accumulating an accumulating accumulation",
                    span,
                );
            }
            *accumulate = true;
            self.bump();
        } else if self.names(0, "eraseable") {
            self.bump();
        } else if self.checks(0, Enabled::Never, "deprecated") {
            self.bump();
            if self.at(K::LParen) {
                self.bump();
                if self.at(K::StringLit) {
                    while self.at(K::StringLit) {
                        self.bump();
                    }
                } else {
                    // `QCC_PR_ParseError (ERR_EXPECTED, "expected string");`
                    self.error(Code::E0204, "expected string");
                }
                self.expect(K::RParen, ")");
            }
        } else {
            let message = format!("Unknown attribute \"{}\"", self.found());
            let span = self.current_span();
            self.warning_at(Code::W0228, message, span);
            while self.current().is_some() && !self.at(K::RBracket) && !self.at(K::Comma) {
                self.bump();
            }
        }
        self.finish_node();
    }

    /// Whether the current name starts a file-level declaration (FTEF-15.1): a qualifier, a
    /// C type word, a tagged type, or a name that may be a type. `starts_declaration` has
    /// already taken a `.`, a `..` and a vanilla type word.
    pub(super) fn fte_starts_declaration(&self) -> bool {
        let Some(tok) = self.current() else {
            return false;
        };
        match tok.kind {
            K::Name if self.decl_qualifier(0) || self.c_type_word(0) || self.at_volatile(0) => true,
            // FTEF-21.9: with `attributes` on, a file-level declaration may start with `[[`.
            K::LBracket => self.flag_ahead(0, FteFlag::Attributes),
            // `struct tag x;`, `enum name x;`: a tagged type names a declaration's type.
            K::Name if self.at_tagged_type(0) => true,
            // `QCC_PR_ParseDefs` reads every definition, so a registered type's name alone starts
            // one (FTEF-20.12).
            K::Name if self.registered_type(0) => true,
            // A type name is followed by the declarator's name, a `*` or a parameter list;
            // a name alone is "not a type" (FTE-21.1).
            K::Name => {
                self.fte_type_name(0)
                    && self
                        .nth(1)
                        .is_some_and(|t| matches!(t.kind, K::Name | K::Star | K::LParen))
            }
            _ => false,
        }
    }

    /// Whether the statement at the current token declares locals (FTEF-16.1): a qualifier,
    /// a C type word, or a name followed by a name, by `*`s and a name, or by a parameter list
    /// and a name. None of these is an expression statement.
    pub(super) fn fte_looks_like_local(&self) -> bool {
        let Some(tok) = self.current() else {
            return false;
        };
        if matches!(tok.kind, K::Dot | K::DotDot) {
            return true;
        }
        if tok.kind != K::Name {
            return false;
        }
        // FTEF-20.14: `QCC_PR_ParseStatement` finds a type by its name with `QCC_TypeForName`,
        // then tests its words exactly, with their keywords on.
        if (self.registered_type(0) && !self.at_scope_resolution(1))
            || self.starts_local_statement(0)
        {
            return true;
        }
        if self.c_type_word(0) || self.at_volatile(0) || self.decl_qualifier(0) {
            // FTEF-2.10, FTEF-2.11, FTEF-19.1: a C type word, `volatile` or a qualifier the test
            // does not take, a `__` word among them, is a name there.
            return false;
        }
        if !self.fte_type_name(0) {
            return false;
        }
        let mut n = 1_usize;
        while self.nth(n).is_some_and(|t| t.kind == K::Star) {
            n = n.saturating_add(1);
        }
        match self.nth(n).map(|t| t.kind) {
            // `Type name`, never an expression.
            Some(K::Name) if n == 1 => true,
            // `Type *name` only where a declarator ends, since `a * b` may be a product.
            Some(K::Name) => self
                .nth(n.saturating_add(1))
                .is_some_and(|t| matches!(t.kind, K::Semicolon | K::Eq | K::Comma | K::LBracket)),
            Some(K::LParen) if n == 1 => self.after_parens_is_name(n),
            _ => false,
        }
    }

    /// Whether the parenthesised list starting at token `n` is followed by a name.
    fn after_parens_is_name(&self, n: usize) -> bool {
        let mut depth = 0_usize;
        let mut ahead = n;
        while let Some(t) = self.nth(ahead) {
            match t.kind {
                K::LParen => depth = depth.saturating_add(1),
                K::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return self
                            .nth(ahead.saturating_add(1))
                            .is_some_and(|t| t.kind == K::Name);
                    }
                }
                K::Semicolon | K::LBrace | K::RBrace => return false,
                _ => {}
            }
            ahead = ahead.saturating_add(1);
        }
        false
    }

    /// `QCC_PR_ParseType` (FTEF-2, FTEF-4, FTEF-7): `.T` and `..T` field types, `struct` or
    /// `union` and a tag, the C type words, a vanilla type or a type name, then `*`s, then, with
    /// `qcfuncs` on, a parameter list (FTEF-21.7).
    pub(super) fn fte_type(&mut self) -> Option<TypeShape> {
        self.fte_type_at(Place::Nested)
    }

    /// The type of a declaration, `QCC_PR_ParseDefs`'s `QCC_PR_ParseType (false, hadmodifier,
    /// !flag_qcfuncs)`: with `qcfuncs` off it takes no `*`, which each declarator takes instead
    /// (FTEF-21.7).
    pub(super) fn fte_decl_type(&mut self) -> Option<TypeShape> {
        self.fte_type_at(Place::Declaration)
    }

    /// The type of a struct's member (FTEF-6.7), which takes no `*`.
    pub(super) fn fte_member_type(&mut self) -> Option<TypeShape> {
        self.fte_type_at(Place::Member)
    }

    fn fte_type_at(&mut self, place: Place) -> Option<TypeShape> {
        if !self.enter() {
            self.bump_error();
            self.leave(1);
            return None;
        }
        let shape = self.fte_type_inner(place);
        self.leave(1);
        shape
    }

    fn fte_type_inner(&mut self, place: Place) -> Option<TypeShape> {
        self.type_derived = false;
        // FTEF-21.7: `ignoreptr`, read once, as the declaration starts.
        let ignore_pointers = match place {
            Place::Declaration => !self.flag_ahead(0, FteFlag::Qcfuncs),
            Place::Member => true,
            Place::Nested => false,
        };
        if self.kw(0, Keyword::Const, "const") {
            // FTEF-2.11: `QCC_PR_ParseType` reads a `const` and ignores it, with a warning.
            self.warn_const();
            self.start_node(K::Qualifier);
            self.bump();
            self.finish_node();
        }
        if self.at_volatile(0) {
            // FTEF-2.11: and a `volatile`, its keyword disabled by default but after `__`.
            self.start_node(K::Qualifier);
            self.bump();
            self.finish_node();
        }
        if self.at(K::DotDot) {
            // `..T`, a field of a field: `QCC_PR_ParseType` takes `..` as two levels ("so we
            // don't end up with the user specifying '. .vector blah'"), as `.` and `.T`.
            self.start_node(K::FieldType);
            self.start_node(K::FieldType);
            self.bump();
            let inner = self.fte_type_at(place);
            self.finish_node();
            self.finish_node();
            return inner.map(|_| TypeShape {
                function: false,
                field: true,
                qcfuncs: false,
            });
        }
        if self.at(K::Dot) {
            self.start_node(K::FieldType);
            self.bump();
            if self.at(K::Star) {
                // `.*float`: a field of pointer to float (FTEF-4.1).
                self.bump();
            }
            let inner = self.fte_type_at(place);
            self.finish_node();
            return inner.map(|_| TypeShape {
                function: false,
                field: true,
                qcfuncs: false,
            });
        }
        let checkpoint = self.checkpoint();
        if !self.fte_base_type() {
            let message = format!("\"{}\" is not a type", self.found());
            self.error(Code::E0203, message);
            return None;
        }
        // FTEF-18.32: each `*` and each parameter list nests the type one level deeper.
        let mut levels = 0_usize;
        while !ignore_pointers && self.at(K::Star) {
            levels = levels.saturating_add(1);
            if !self.enter() {
                break;
            }
            self.builder.start_node_at(checkpoint, K::PointerType);
            self.bump();
            if self.kw(0, Keyword::Const, "const") {
                // FTEF-2.11: `T *const` is the pointer, with the warning.
                self.warn_const();
                self.bump();
            }
            self.finish_node();
        }
        // FTEF-21.7: `if (flag_qcfuncs && QCC_PR_CheckToken ("("))`, at the token after the
        // type's words and `*`s.
        let qcfuncs = self.flag_ahead(0, FteFlag::Qcfuncs);
        let function = (qcfuncs && self.at_function_params()) || self.type_function;
        // FTEF-18.15: `QCC_PR_ParseDefs` loops `while(QCC_PR_CheckToken ("("))`, so each
        // further parameter list makes a function returning the type so far.
        // A first list of a name that is no type is "expected function arg list", which the
        // semantic model reports as the name's ("is not a type"); no list after it is counted.
        let named_first = self.at(K::LParen)
            && self.nth(1).is_some_and(|t| t.kind == K::Name)
            && self.nth(2).is_some_and(|t| t.kind == K::RParen)
            && self.type_word(1).is_none()
            && !self.c_type_word(1)
            && !self.declared_type(1);
        let mut lists = 0_usize;
        while function && self.at_function_params() && !(levels > 0 && self.at_parenthesised_name())
        {
            lists = lists.saturating_add(1);
            levels = levels.saturating_add(1);
            if !self.enter() {
                break;
            }
            self.builder.start_node_at(checkpoint, K::FunctionType);
            self.fte_param_list();
            self.finish_node();
        }
        self.leave(levels);
        self.type_derived = levels > 0;
        self.type_lists = if named_first { lists.min(1) } else { lists };
        Some(TypeShape {
            function,
            field: false,
            qcfuncs,
        })
    }

    /// What `QCC_PR_ParseType` takes after a struct, union, class, enum or accessor written as a
    /// declaration's type, before `QCC_PR_ParseDefs`'s declarators (FTEF-6.8, FTEF-21.7): with
    /// `qcfuncs` on, the `*`s (`if (!ignoreptr) while (QCC_PR_CheckToken("*"))`), each as a
    /// token of the declaration, and the parameter lists (`if (flag_qcfuncs &&
    /// QCC_PR_CheckToken ("("))`), each a `FunctionType` node of its own; with it off, nothing.
    pub(super) fn type_decl_tail(&mut self) -> TypeShape {
        let qcfuncs = self.flag_ahead(0, FteFlag::Qcfuncs);
        while qcfuncs && self.at(K::Star) {
            self.bump();
            if self.kw(0, Keyword::Const, "const") {
                // FTEF-2.11: `T *const` is the pointer, with the warning.
                self.warn_const();
                self.bump();
            }
        }
        let function = qcfuncs && self.at_function_params();
        while function && self.at_function_params() {
            self.start_node(K::FunctionType);
            self.fte_param_list();
            self.finish_node();
        }
        TypeShape {
            function,
            field: false,
            qcfuncs,
        }
    }

    /// FTEF-15.9: whether `(name)` starts here with `qcfuncs` on, `name` no type, and what ends a
    /// declarator after it, `;`, `=`, `,` or `[`: after a function type no parameter list, which
    /// would leave the declarator no name, but C's parenthesised name
    /// (`QCC_PR_ParseFunctionType` finds no type, and the C-style branch reads the name with no
    /// `*`). With `qcfuncs` off the list is read as a QC one and the name is missing.
    pub(super) fn at_parenthesised_name(&self) -> bool {
        self.at(K::LParen)
            && self.flag_ahead(0, FteFlag::Qcfuncs)
            && self.nth(1).is_some_and(|t| t.kind == K::Name)
            && self.type_word(1).is_none()
            && !self.c_type_word(1)
            && !self.declared_type(1)
            && self.nth(2).is_some_and(|t| t.kind == K::RParen)
            && self
                .nth(3)
                .is_some_and(|t| matches!(t.kind, K::Semicolon | K::Eq | K::Comma | K::LBracket))
    }

    /// Whether a type's parameter list starts here: a `(` that opens no `(*name)` (FTEF-15.3).
    fn at_function_params(&self) -> bool {
        self.at(K::LParen) && !self.nth(1).is_some_and(|t| t.kind == K::Star)
    }

    /// Whether the token `n` ahead starts a type that `QCC_PR_ParseType` reads by its keyword
    /// alone (FTEF-9.8): `const`, `volatile`, `accessor`, `class` with `qcfuncs` on, `enum`,
    /// `enumflags`, `union` and `struct`.
    pub(super) fn type_form(&self, n: usize) -> bool {
        self.kw(n, Keyword::Const, "const")
            || self.at_volatile(n)
            || self.kw(n, Keyword::Accessor, "accessor")
            || self.at_class_keyword(n)
            || self.at_tagged_type(n)
    }

    /// Whether a tagged type's keyword is the token `n` ahead, with the keywords in force
    /// (FTEF-20.8): `enum`, `enumflags`, `union` or `struct`.
    pub(super) fn at_tagged_type(&self, n: usize) -> bool {
        self.at_enum_keyword(n) || self.at_struct_keyword(n)
    }

    /// Whether `enum` or `enumflags` is the token `n` ahead, with its keyword (FTEF-20.8).
    pub(super) fn at_enum_keyword(&self, n: usize) -> bool {
        self.kw(n, Keyword::Enum, "enum") || self.kw(n, Keyword::Enumflags, "enumflags")
    }

    /// Whether `struct` or `union` is the token `n` ahead, with its keyword (FTEF-20.8).
    pub(super) fn at_struct_keyword(&self, n: usize) -> bool {
        self.kw(n, Keyword::Union, "union") || self.kw(n, Keyword::Struct, "struct")
    }

    /// The base of a type: a `SimpleType` node holding a vanilla keyword, a run of C type
    /// words, `struct`/`union` and a tag, or a type name. False, consuming nothing, when the
    /// token starts none. Each keyword is read with the keywords in force (FTEF-20.8).
    fn fte_base_type(&mut self) -> bool {
        self.type_function = false;
        let Some(tok) = self.current() else {
            return false;
        };
        if tok.kind != K::Name {
            return false;
        }
        if self.at_enum_keyword(0) {
            // FTEF-8.2: an enum written as a type, `enumflags:float{A, B} flags`. FTEF-8.4: it
            // always has its body; an enum is named by its type name alone.
            self.start_node(K::SimpleType);
            self.enum_body();
            self.finish_node();
            return true;
        }
        if self.kw(0, Keyword::Accessor, "accessor") {
            // FTEF-10.1: an accessor, defined or named, as a type anywhere a type stands.
            self.start_node(K::SimpleType);
            self.start_node(K::AccessorDecl);
            self.accessor_type();
            self.finish_node();
            self.finish_node();
            return true;
        }
        if self.at_class_keyword(0) {
            // FTEF-9.8: `class Name` names a class, and `class Name { ... }` defines one, as a
            // type anywhere a type stands.
            self.start_node(K::SimpleType);
            self.start_node(K::ClassDecl);
            self.class_type();
            self.finish_node();
            self.finish_node();
            return true;
        }
        if self.at_struct_keyword(0)
            && self.nth(1).is_some_and(|t| t.kind == K::Name)
            && !self
                .nth(2)
                .is_some_and(|t| matches!(t.kind, K::LBrace | K::Colon))
        {
            // FTEF-6.1: `struct tag` names a struct by its tag; the tag becomes a type there
            // if none is.
            self.start_node(K::SimpleType);
            let keyword = if self.kw(0, Keyword::Union, "union") {
                K::UnionKw
            } else {
                K::StructKw
            };
            self.bump_as(keyword);
            let tag = self.found();
            if self.name() {
                self.declare_type(tag);
            }
            self.finish_node();
            return true;
        }
        if self.at_struct_keyword(0) {
            // FTEF-6.4: a struct written as a type, `struct tag { ... } *field;`, or named by its
            // tag, `struct tag x;` (FTEF-6.1).
            self.start_node(K::SimpleType);
            self.struct_type();
            self.finish_node();
            return true;
        }
        if self.c_type_word(0) {
            // FTEF-2.8: the type is the words the loop takes; the next word is the name.
            let mut words = Vec::new();
            while let Some(t) = self.nth(words.len()).filter(|t| t.kind == K::Name) {
                words.push(t.text.clone());
            }
            let context = self.c_context(0);
            let (taken, ty) = CWords::read(words.iter().map(String::as_str), context);
            let span = self.current().map(|t| t.span);
            self.start_node(K::SimpleType);
            for _ in 0..taken {
                self.bump();
            }
            self.finish_node();
            // FTEF-2.9: a narrowed type is warned about.
            if let (Some(ty), Some(span)) = (ty, span) {
                for warning in ty.warnings() {
                    self.warning_at(Code::W0226, warning, span);
                }
            }
            return true;
        }
        if let Some(keyword) = self.type_word(0) {
            self.start_node(K::SimpleType);
            // A vanilla type word is a type of its own name, which `QCC_PR_ParseType` finds
            // with `QCC_TypeForName` before its C type words, so no C word joins it (FTEF-2.4).
            self.bump_as(keyword);
            self.finish_node();
            return true;
        }
        if !self.fte_type_name(0) {
            return false;
        }
        // FTEF-7.5: a typedef of a function type is one (`type->type == ev_function` after
        // `QCC_TypeForName`), so a body, state or builtin may follow its declarator.
        self.type_function = self.declared_function(0);
        self.start_node(K::SimpleType);
        self.bump();
        self.finish_node();
        true
    }

    /// `QCC_PR_ParseFunctionType` (FTEF-15.3, FTEF-15.4): `( )`, or parameters separated by
    /// `,`, each with its qualifiers, a type, an optional name or `(*name)(params)`, array
    /// suffixes and a default value, and a `...` that ends the list, optionally with a count
    /// name.
    /// The names it gives its parameters are the ones a body after it defines (FTEF-20.4).
    pub(super) fn fte_param_list(&mut self) {
        self.start_node(K::ParamList);
        let start = self.current_span().start;
        self.bump();
        let mut names = Vec::new();
        if self.at(K::RParen) {
            self.bump();
            self.finish_node();
            self.params = names;
            return;
        }
        let mut count = 0_usize;
        loop {
            if self.at(K::Ellipsis) {
                self.bump();
                if self.at(K::Name) {
                    // `...count`: the name of the argument count (FTEF-15.3).
                    self.bump();
                }
                break;
            }
            let before = self.pos;
            names.extend(self.fte_param(count == 0));
            count = count.saturating_add(1);
            if self.pos == before {
                self.bump_error();
            }
            if !self.at(K::Comma) {
                break;
            }
            self.bump();
        }
        let closed = self.expect(K::RParen, ")");
        if closed && count > FTE_MAX_PARMS {
            let span = Span::new(start, self.last_end);
            self.error_at(Code::E0505, "too many parameters", span);
        }
        self.finish_node();
        self.params = names;
    }

    /// One parameter of [`Parser::fte_param_list`], the list's first when `first` says: its
    /// name, if it has one, and whether its type is a function's.
    fn fte_param(&mut self, first: bool) -> Option<(String, bool)> {
        self.start_node(K::Param);
        let mut qualified = false;
        while self.param_qualifier(0) {
            qualified = true;
            self.start_node(K::Qualifier);
            self.bump();
            self.finish_node();
        }
        // FTEF-21.8: with `qcfuncs` off a first parameter that is no type is an `int` named by
        // its word (`maybename`, `if (!flag_qcfuncs) t = type_integer;`). A lone name is read
        // so; one a typedef names is not told apart.
        let lone_name = first
            && !qualified
            && !self.flag_ahead(0, FteFlag::Qcfuncs)
            && self
                .current()
                .is_some_and(|t| t.kind == K::Name && self.type_word(0).is_none())
            && !self.c_type_word(0)
            && !self.at_tagged_type(0)
            && self
                .nth(1)
                .is_some_and(|t| matches!(t.kind, K::RParen | K::Comma));
        if lone_name {
            let name = self.found();
            self.name();
            self.finish_node();
            return Some((name, false));
        }
        let Some(ty) = self.fte_type() else {
            self.skip_param();
            self.finish_node();
            return None;
        };
        // `void() *a`: `QCC_PR_ParseFunctionType` reads `*`s after a parameter's type, a
        // function type's included (FTEF-15.3).
        while ty.function && self.at(K::Star) {
            self.bump();
        }
        let mut named = None;
        if self.at(K::Amp) {
            // `type &name`: an out parameter, C++'s spelling (FTEF-15.3).
            self.bump();
        }
        if self.at(K::Ellipsis) {
            // `type ...`: the variadic arguments are of this type.
            self.bump();
            self.finish_node();
            return None;
        }
        if self.at(K::LParen) && self.nth(1).is_some_and(|t| t.kind == K::Star) {
            // `(*name)(params)`: a function-pointer parameter.
            self.bump();
            self.bump();
            let name = self.found();
            if self.name() {
                named = Some((name, true));
            }
            if self.at(K::LBracket) {
                self.array_suffix();
            }
            self.expect(K::RParen, ")");
            if self.at(K::LParen) {
                self.fte_param_list();
            }
        } else if self.at(K::Name) {
            let name = self.found();
            if self.name() {
                named = Some((name, ty.function));
            }
        }
        while self.at(K::LBracket) {
            self.array_suffix();
        }
        if self.at(K::Eq) {
            self.start_node(K::DefaultValue);
            self.bump();
            self.expression();
            self.finish_node();
        }
        self.finish_node();
        named
    }

    // ---- file-level constructs ----

    /// `class Name [: Parent] { members }` or `class Name` as a definition's type: the qualifiers,
    /// `QCC_PR_ParseEntClass`'s type, and the declarators that follow it as they follow any type
    /// (FTEF-9.1, FTEF-9.8).
    pub(super) fn class_decl(&mut self) {
        self.start_node(K::ClassDecl);
        // `static class ...`: `QCC_PR_ParseDefs` reads the qualifiers, then the class as the
        // declaration's type.
        self.fte_qualifiers();
        self.class_type();
        let shape = self.type_decl_tail();
        self.declarators(shape, Site::File);
        self.finish_node();
    }

    /// `QCC_PR_ParseEntClass` (FTEF-9.1): `class`, a name, a parent after `:`, which needs the
    /// body, and the body, or nothing after a name, which names the class (FTEF-9.8).
    pub(super) fn class_type(&mut self) {
        self.bump_as(K::ClassKw);
        let declared = self.found();
        if !self.name() {
            return;
        }
        self.declare_type(declared.clone());
        // The class being read around this one comes back once its body is read.
        let outer = self.class_name.replace(declared);
        self.class_body();
        self.class_name = outer;
    }

    /// The rest of a class after its name: a parent, which needs the body, and the body.
    fn class_body(&mut self) {
        let mut parent = false;
        if self.at(K::Colon) {
            self.bump();
            parent = true;
            if !self.name() {
                return;
            }
        }
        let opened = if parent {
            self.expect(K::LBrace, "{")
        } else if self.at(K::LBrace) {
            self.bump();
            true
        } else {
            false
        };
        if !opened {
            return;
        }
        if self.bodies > 0 {
            // `if (pr_scope) QCC_PR_ParseError(ERR_REDECLARATION, "Declaration of class %s%s%s
            // within function", ...)`
            self.error(Code::E0253, "Declaration of class within function");
        }
        if self.at(K::Comma) {
            // `if (QCC_PR_CheckToken(",")) QCC_PR_ParseError(ERR_NOTANAME, "member missing name");`
            self.error(Code::E0201, "member missing name");
        }
        while !self.at(K::RBrace) && self.current().is_some() {
            if self.lost_brace() {
                // FTEF-18.27: a class that lost its `}` ends at the next declaration line.
                break;
            }
            let before = self.pos;
            self.start_item();
            self.class_member();
            if self.pos == before {
                self.bump_error();
            }
        }
        self.expect(K::RBrace, "}");
    }

    /// Whether a keyword that starts a file-level construct of the full dialect is here:
    /// `typedef`, `class`, `accessor`, `enum`, `enumflags`, `struct` or `union` (FTEF-18.27).
    pub(super) fn at_fte_construct(&self) -> bool {
        self.current().is_some_and(|t| {
            t.kind == K::Name
                && matches!(
                    t.text.as_str(),
                    "typedef" | "class" | "accessor" | "enum" | "enumflags" | "struct" | "union"
                )
        })
    }

    /// Whether an `enum` or `enumflags` with its body starts here (FTE-15, FTEF-8): the keyword
    /// and `{`, `:`, `class`, or a name and `{` or `:`. `enum Name x` names the enum as a type.
    pub(super) fn at_enum_body(&self) -> bool {
        if !self.at_enum_keyword(0) {
            return false;
        }
        match self.nth(1).map(|t| t.kind) {
            Some(K::LBrace | K::Colon) => true,
            Some(K::Name) => {
                // `strictenum = QCC_PR_CheckName("class");`
                self.names(1, "class")
                    || self
                        .nth(2)
                        .is_some_and(|t| matches!(t.kind, K::LBrace | K::Colon))
            }
            _ => false,
        }
    }

    /// Whether `class` is the token `n` ahead, with its keyword, and `qcfuncs` on at it
    /// (`if (flag_qcfuncs && QCC_PR_CheckKeyword (keyword_class, "class"))`, FTEF-20.8).
    pub(super) fn at_class_keyword(&self, n: usize) -> bool {
        self.kw(n, Keyword::Class, "class")
            && self
                .nth(n)
                .is_some_and(|t| self.lexed_with(t.span.start, LexFlag::QcFuncs))
    }

    /// Whether an accessor starts here, after any declaration qualifiers (FTEF-10.1).
    pub(super) fn at_accessor(&self) -> bool {
        let mut n = 0_usize;
        while self.decl_qualifier(n) {
            n = n.saturating_add(1);
        }
        self.kw(n, Keyword::Accessor, "accessor")
    }

    /// Whether a class starts here, after any declaration qualifiers (FTEF-9.1).
    pub(super) fn at_class(&self) -> bool {
        let mut n = 0_usize;
        while self.decl_qualifier(n) {
            n = n.saturating_add(1);
        }
        self.at_class_keyword(n)
    }

    /// One member of a class body (FTEF-9.2 to FTEF-9.4, FTEF-10.3): an access or default
    /// label, a `get`/`set` accessor member, or qualifiers and a declaration of fields or
    /// member functions, which may have bodies.
    fn class_member(&mut self) {
        // FTEF-9.9: `public` and `private` take a `:` (`QCC_PR_Expect(":")`), `protected` two;
        // `virtual` and `nonvirtual` are a label only when a `:` follows. The member follows its
        // labels, which `QCC_PR_ParseEntClass`'s loop of keywords keeps reading.
        let mut labels = 0_usize;
        loop {
            let labelled = |parser: &Self, word: &str| parser.checks(0, Enabled::Always, word);
            let colons = if labelled(self, "public") || labelled(self, "private") {
                1_usize
            } else if labelled(self, "protected") {
                2
            } else if (labelled(self, "virtual") || labelled(self, "nonvirtual"))
                && self.nth(1).is_some_and(|t| t.kind == K::Colon)
            {
                1
            } else {
                break;
            };
            self.start_node(K::AccessLabel);
            self.bump();
            for _ in 0..colons {
                self.expect(K::Colon, ":");
            }
            self.finish_node();
            labels = labels.saturating_add(1);
        }
        if labels > 0 && self.at(K::RBrace) {
            let message = format!("\"{}\" is not a type", self.found());
            self.error(Code::E0203, message);
            return;
        }
        // FTEF-10.10: `get` and `set` are among the qualifiers `QCC_PR_ParseClass` reads in any
        // order, and either makes the member an accessor's.
        let get_set =
            |n| self.checks(n, Enabled::Always, "get") || self.checks(n, Enabled::Always, "set");
        let mut ahead = 0_usize;
        let mut accessor = false;
        while self.member_qualifier(ahead) || get_set(ahead) {
            accessor |= get_set(ahead);
            ahead = ahead.saturating_add(1);
        }
        if accessor {
            self.accessor_member(Member::Class);
            return;
        }
        self.start_node(K::Declaration);
        while self.member_qualifier(0) {
            self.start_node(K::Qualifier);
            match self.current().map(|t| t.text.clone()).as_deref() {
                Some("const") => self.bump_as(K::ConstKw),
                Some("var") => self.bump_as(K::VarKw),
                _ => self.bump(),
            }
            self.finish_node();
        }
        // `struct float` is `float`, the type the name finds (FTEF-6.8), no struct.
        let struct_keyword =
            self.at_struct_keyword(0) && !(self.type_word(1).is_some() || self.c_type_word(1));
        // FTEF-10.6: an accessor declared with its base where a member's type stands ends
        // FTEQCC; the member is refused here.
        let accessor_based = self.kw(0, Keyword::Accessor, "accessor")
            && self.nth(2).is_some_and(|t| t.kind == K::Colon);
        let head = self.found();
        let accessor = self.nth(1).map(|t| t.text.clone()).unwrap_or_default();
        let ty = self.fte_type();
        if accessor_based {
            let class = self.class_name.clone().unwrap_or_default();
            let message = format!("Accessor {accessor} declared with its base in class {class}");
            self.error(Code::E0258, message);
        }
        if struct_keyword && !self.type_derived {
            // `if (basetype->type == ev_struct || basetype->type == ev_union)
            // QCC_PR_ParseError(ERR_INTERNAL, "Struct or union in class %s", classname);`; a
            // pointer to one is a pointer.
            self.error(Code::E0254, "Struct or union in class");
        }
        match ty {
            // `Name(void);`: a constructor with no return type (FTEF-9.4).
            Some(ty) if ty.function && self.at(K::Semicolon) => {
                // `!strcmp(parmname, classname)`: the constructor's name is the class's, exactly,
                // under `-Fcaseinsens` too.
                if self.class_name.as_deref() != Some(head.as_str()) {
                    let message = format!("\"{head}\" is not a type");
                    self.error(Code::E0203, message);
                }
                self.bump();
            }
            // A member's recovery stops at the `}` that closes the class.
            Some(ty) => self.declarators(ty, Site::Member),
            None => self.recover(true),
        }
        self.finish_node();
    }

    /// `struct Name { fields };`, `union Name { ... };` or the forward `struct Name;` at file
    /// level (FTEF-6.1, FTEF-6.2).
    pub(super) fn struct_decl(&mut self) {
        self.start_node(K::StructDecl);
        self.struct_type();
        // FTEF-6.1: `struct { ... } a[4], *p;` declares variables of the struct too, as any type
        // is followed by declarators.
        let shape = self.type_decl_tail();
        self.declarators(shape, Site::File);
        self.finish_node();
    }

    /// `accessor Name [: Base] [{ members }]` and the declarators after it (FTEF-10.1): the
    /// qualifiers, `QCC_PR_ParseType`'s accessor, and the declarators that follow any type.
    pub(super) fn accessor_decl(&mut self) {
        self.start_node(K::AccessorDecl);
        self.fte_qualifiers();
        self.accessor_type();
        let shape = self.type_decl_tail();
        self.declarators(shape, Site::File);
        self.finish_node();
    }

    /// `accessor Name [: Base] [{ members }]` (`QCC_PR_ParseType`, FTEF-10.1).
    pub(super) fn accessor_type(&mut self) {
        self.bump_as(K::AccessorKw);
        let declared = self.found();
        if self.name() {
            self.declare_type(declared);
        }
        if self.at(K::Colon) {
            self.bump();
            self.fte_type();
        }
        if self.at(K::LBrace) {
            self.bump();
            // FTEF-18.27: an accessor that lost its `}` ends at the next declaration line.
            while !self.at(K::RBrace) && self.current().is_some() && !self.lost_brace() {
                let before = self.pos;
                self.start_item();
                self.accessor_member(Member::Accessor);
                if self.at(K::Comma) || self.at(K::Semicolon) {
                    self.bump();
                }
                if self.pos == before {
                    self.bump_error();
                }
            }
            self.expect(K::RBrace, "}");
        }
    }

    /// One accessor member (`QCC_PR_ParseAccessorMember`, FTEF-10.2): `[inline] get|set`, a
    /// type, an optional name, an optional `[IndexType [name]]`, and `= function` or
    /// `= { body }`. In a class its words are `QCC_PR_CheckKeyword(1, ...)`'s, in an accessor
    /// `QCC_PR_CheckName`'s (FTEF-20.11).
    fn accessor_member(&mut self, member: Member) {
        self.start_node(K::AccessorMember);
        let word = |parser: &Self, word: &str| match member {
            Member::Class => parser.checks(0, Enabled::Always, word),
            Member::Accessor => parser.names(0, word),
        };
        let get_set = |parser: &Self| word(parser, "get") || word(parser, "set");
        let mut accessor = false;
        match member {
            Member::Class => {
                while self.member_qualifier(0) || get_set(self) {
                    accessor |= get_set(self);
                    self.bump();
                }
            }
            Member::Accessor => {
                if word(self, "inline") {
                    self.bump();
                }
                if get_set(self) {
                    accessor = true;
                    self.bump();
                }
            }
        }
        if !accessor {
            self.expected("get or set");
            self.recover(true);
            self.finish_node();
            return;
        }
        // FTEF-10.9: `QCC_PR_ParseAccessorMember` reads a type's name and its `*`s, so a
        // parameter list after it is where the `=` should be.
        let paren = self.nth(1).filter(|t| t.kind == K::LParen).map(|t| t.span);
        if let Some(shape) = self.fte_type() {
            if shape.function
                && let Some(span) = paren
            {
                self.error_at(Code::E0204, "expected =, found (", span);
                self.recover(true);
                self.finish_node();
                return;
            }
            if self.at(K::Name) {
                self.name();
            }
            if self.at(K::LBracket) {
                self.start_node(K::ArraySuffix);
                self.bump();
                if self.fte_type().is_some() && self.at(K::Name) {
                    self.name();
                }
                self.expect(K::RBracket, "]");
                self.finish_node();
            }
            // FTEF-10.11: `QCC_PR_Expect("=")`, then a body or a function's name.
            if self.expect(K::Eq, "=") {
                if self.at(K::LBrace) {
                    self.start_node(K::FunctionBody);
                    self.block(true);
                    self.finish_node();
                } else {
                    self.expression();
                }
            }
        }
        if self.at(K::Semicolon) {
            self.bump();
        }
        self.finish_node();
    }

    // ---- terms ----

    /// Whether the `(` here opens a cast (FTEF-16.2): a type word, a `.` or a C type word
    /// follows; or a type name and `*`s or a parameter list and then `)`; or a lone name in
    /// parentheses followed by a token that starts a term.
    pub(super) fn fte_at_cast(&self) -> bool {
        let Some(first) = self.nth(1) else {
            return false;
        };
        if matches!(first.kind, K::Dot | K::DotDot) {
            return true;
        }
        if first.kind != K::Name {
            return false;
        }
        if self.type_word(1).is_some() || self.c_type_word(1) || self.type_form(1) {
            return true;
        }
        if !self.fte_type_name(1) {
            return false;
        }
        let mut n = 2_usize;
        while self.nth(n).is_some_and(|t| t.kind == K::Star) {
            n = n.saturating_add(1);
        }
        if n > 2 {
            return self.nth(n).is_some_and(|t| t.kind == K::RParen)
                && self.starts_term(n.saturating_add(1));
        }
        match self.nth(2).map(|t| t.kind) {
            Some(K::RParen) => self.starts_term(3),
            // `(Type(params))`: a function type, whose list starts with a parameter (a type
            // word, `...`, `)` or a name followed by a name or `*`), closed after its list. A
            // call in parentheses, `(visible(head))`, has an expression there.
            Some(K::LParen) if self.starts_param(3) => {
                let mut depth = 0_usize;
                let mut ahead = 2_usize;
                while let Some(t) = self.nth(ahead) {
                    match t.kind {
                        K::LParen => depth = depth.saturating_add(1),
                        K::RParen => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                let close = ahead.saturating_add(1);
                                return self.nth(close).is_some_and(|t| t.kind == K::RParen)
                                    && self.starts_term(close.saturating_add(1));
                            }
                        }
                        K::Semicolon | K::LBrace => return false,
                        _ => {}
                    }
                    ahead = ahead.saturating_add(1);
                }
                false
            }
            _ => false,
        }
    }

    /// Whether the token `n` ahead can start a term, as the operand of a cast does.
    fn starts_term(&self, n: usize) -> bool {
        self.nth(n).is_some_and(|t| {
            matches!(
                t.kind,
                K::Name
                    | K::FloatLit
                    | K::VectorLit
                    | K::StringLit
                    | K::FrameRef
                    | K::LParen
                    | K::Not
                    | K::Tilde
                    | K::LBracket
                    | K::Amp
                    | K::ColonColon
            )
        })
    }

    /// Whether the token `n` ahead can start a parameter list's contents.
    fn starts_param(&self, n: usize) -> bool {
        let Some(t) = self.nth(n) else {
            return false;
        };
        match t.kind {
            K::RParen | K::Ellipsis | K::Dot | K::DotDot => true,
            K::Name => {
                self.type_word(n).is_some()
                    || self.c_type_word(n)
                    || self.param_qualifier(n)
                    || self
                        .nth(n.saturating_add(1))
                        .is_some_and(|t| matches!(t.kind, K::Name | K::Star))
            }
            _ => false,
        }
    }

    /// `(type) e`, its operand at `UNARY_PRIORITY`, or `(type){ ... }` (FTEF-16.2).
    pub(super) fn fte_cast(&mut self) {
        self.start_node(K::CastExpr);
        self.bump();
        self.fte_type();
        self.expect(K::RParen, ")");
        if self.at(K::LBrace) {
            self.aggregate_init();
        } else {
            self.operand(UNARY_PRIORITY, false);
        }
    }

    /// `[x, y, z]`: a vector of one to three expressions, the rest 0 (FTEF-16.3).
    pub(super) fn vector_expr(&mut self) {
        self.start_node(K::VectorExpr);
        self.bump();
        for component in 0..3 {
            if component > 0 {
                if !self.at(K::Comma) {
                    break;
                }
                self.bump();
            }
            self.expression();
        }
        self.expect(K::RBracket, "]");
    }

    /// `sizeof` and a type or an expression, with or without parentheses (FTEF-18.12):
    /// `QCC_PR_RefTerm` reads `pbool bracket = QCC_PR_CheckToken("(");`, then a type when one
    /// follows, else `QCC_PR_RefExpression(&refbuf, TOP_PRIORITY, 0)`, then the `)`.
    pub(super) fn sizeof_expr(&mut self) {
        self.start_node(K::SizeofExpr);
        self.bump_as(K::SizeofKw);
        let bracket = self.at(K::LParen);
        if bracket {
            self.bump();
        }
        if self.sizeof_type_follows(bracket) {
            self.fte_type();
        } else {
            self.sequence();
        }
        if bracket {
            self.expect(K::RParen, ")");
        }
    }

    /// Whether a type follows `sizeof` (FTEF-18.12): a type word, a C type word, a field
    /// type, a tagged type, or, inside parentheses, a name and `*`s before the `)`. FTEQCC
    /// knows its type names; a lone name here is read as a value, which a type's name also is
    /// (FTEF-17.1).
    fn sizeof_type_follows(&self, bracket: bool) -> bool {
        let Some(tok) = self.current() else {
            return false;
        };
        match tok.kind {
            K::Dot | K::DotDot => true,
            K::Name => {
                if self.registered_type(0) || self.c_type_word(0) || self.type_form(0) {
                    return true;
                }
                if !bracket || !self.fte_type_name(0) {
                    return false;
                }
                let mut n = 1_usize;
                while self.nth(n).is_some_and(|t| t.kind == K::Star) {
                    n = n.saturating_add(1);
                }
                n > 1 && self.nth(n).is_some_and(|t| t.kind == K::RParen)
            }
            _ => false,
        }
    }

    // ---- statements ----

    /// The state header (FTEF-18.3, FTEF-18.28): `[frame, next]` with an expression for each,
    /// the `,` a warning when missing; or a frame cycle `[++ first .. last]` or
    /// `[-- first .. last]`, `(w)` after the `++` or `--` cycling `weaponframe`.
    pub(super) fn fte_state(&mut self) {
        self.start_node(K::State);
        self.bump();
        if self.at(K::PlusPlus) || self.at(K::MinusMinus) {
            self.frame_cycle();
        } else {
            self.expression();
            if self.at(K::Comma) {
                self.bump();
            } else {
                // `QCC_PR_ParseWarning(WARN_UNEXPECTEDPUNCT, "missing comma in state
                // definition");`
                let span = self.current_span();
                self.warning_at(Code::W0205, "missing comma in state definition", span);
            }
            // A name alone is the next think function, which resolution declares when it is
            // new (QC-13.6), a function definition named as `QCC_PR_DummyDef` reads it
            // (FTEF-20.4); any other expression is a value of the body.
            if self.at(K::Name) && self.nth(1).is_some_and(|t| t.kind == K::RBracket) {
                let name = self.found();
                self.name();
                self.define(&name, true);
            } else {
                self.expression();
            }
        }
        if !self.expect(K::RBracket, "]") {
            // The rest of a header that went wrong, up to its `]` or the body's `{`.
            let mut open = false;
            while !self.at(K::RBracket) && !self.at(K::LBrace) && self.current().is_some() {
                if !open {
                    self.start_node(K::Error);
                    open = true;
                }
                self.bump();
            }
            if open {
                self.finish_node();
            }
            if self.at(K::RBracket) {
                self.bump();
            }
        }
        self.finish_node();
    }

    /// `++ first .. last` or `-- first .. last`, after the `[` (FTEF-18.28). Two constant
    /// frames against the direction are "Forwards State Cycle with backwards range", an
    /// error as FTEQCC raises it (`ERR_STATETYPEMISMATCH`).
    fn frame_cycle(&mut self) {
        let forwards = self.at(K::PlusPlus);
        self.bump();
        if self.at(K::LParen) {
            // `(w)` or `(W)`: `if (!QCC_PR_CheckToken("w")) QCC_PR_Expect("W");`.
            self.bump();
            if !(self.at_word("w") || self.at_word("W")) {
                self.expected("W");
            }
            if self.at(K::Name) {
                self.bump();
            }
            self.expect(K::RParen, ")");
        }
        let start = self.current_span().start;
        let first = self.constant_frame();
        self.expect(K::DotDot, "..");
        let last = self.constant_frame();
        if let (Some(first), Some(last)) = (first, last) {
            let backwards = if forwards { first > last } else { first < last };
            if backwards {
                let span = Span::new(start, self.last_end);
                self.error_at(
                    Code::E0212,
                    "Forwards State Cycle with backwards range",
                    span,
                );
            }
        }
    }

    /// One frame of a cycle: an expression, and its value when it is a single number.
    fn constant_frame(&mut self) -> Option<f32> {
        let value = self
            .current()
            .filter(|t| t.kind == K::FloatLit)
            .and_then(|t| t.value);
        let before = self.pos;
        self.expression();
        value.filter(|_| self.pos == before.saturating_add(1))
    }

    /// `__exit;` (FTEF-20.5): `QCC_PR_Statement (&pr_opcodes[OP_DONE], ...)` and
    /// `QCC_PR_Expect (";")`.
    pub(super) fn exit_stmt(&mut self) {
        self.start_node(K::ExitStmt);
        self.bump_as(K::ExitKw);
        self.end_statement();
        self.finish_node();
    }

    /// `loop s` (FTEF-20.5): the statement, run again until a `break`.
    pub(super) fn loop_stmt(&mut self) {
        self.start_node(K::LoopStmt);
        self.bump_as(K::LoopKw);
        self.statement();
        self.finish_node();
    }

    /// `thinktime e : t;` (FTEF-20.5): `QCC_PR_Expression (TOP_PRIORITY, 0)`, a `:`, another
    /// and a `;`.
    pub(super) fn thinktime_stmt(&mut self) {
        self.start_node(K::ThinktimeStmt);
        self.bump_as(K::ThinktimeKw);
        self.expression();
        if self.expect(K::Colon, ":") {
            self.expression();
            self.end_statement();
        } else {
            self.recover(true);
        }
        self.finish_node();
    }

    /// `state [frame, next];` (FTEF-20.5): `QCC_PR_Expect("["); QCC_PR_ParseState();
    /// QCC_PR_Expect(";");`, the state header of a function's (FTEF-18.3).
    pub(super) fn state_stmt(&mut self) {
        self.start_node(K::StateStmt);
        self.bump_as(K::StateKw);
        if self.at(K::LBracket) {
            self.fte_state();
            self.end_statement();
        } else {
            self.expected("[");
            self.recover(true);
        }
        self.finish_node();
    }

    /// `using (a, b = c) s` or `using a, b = c;` (FTEF-20.5, `QCC_PR_ParseStatement_Using`):
    /// each name with an optional `=` and the alias's name, separated by `,`; in parentheses
    /// a statement follows.
    pub(super) fn using_stmt(&mut self) {
        self.start_node(K::UsingStmt);
        self.bump_as(K::UsingKw);
        let block = self.at(K::LParen);
        if block {
            self.bump();
        }
        loop {
            self.start_node(K::UsingAlias);
            let mut alias = self.found();
            let mut named = self.name();
            if named && self.at(K::Eq) {
                self.bump();
                alias = self.found();
                named = self.name();
            }
            // `QCC_PR_DummyDef(def->type, name, ...)` makes the alias.
            if named {
                self.define(&alias, false);
            }
            self.finish_node();
            if !self.at(K::Comma) {
                break;
            }
            self.bump();
        }
        if block {
            if self.expect(K::RParen, ")") {
                self.statement();
            }
        } else {
            self.end_statement();
        }
        self.finish_node();
    }

    /// `asm` and an instruction, or a block of them in `{ }` (FTEF-20.5, `QCC_PR_ParseAsm`).
    pub(super) fn asm_stmt(&mut self) {
        self.start_node(K::AsmStmt);
        self.bump_as(K::AsmKw);
        if self.at(K::LBrace) {
            self.bump();
            while !self.at(K::RBrace) && self.current().is_some() {
                let before = self.pos;
                self.asm_instruction();
                if self.pos == before {
                    self.bump_error();
                }
            }
            self.expect(K::RBrace, "}");
        } else {
            self.asm_instruction();
        }
        self.finish_node();
    }

    /// One instruction of an `asm` statement: `local` and a declaration, or an opcode named by
    /// its `opname` exactly and its operands up to the `;`, which are code generation's (L-01);
    /// any other token is "Bad op code name NAME" (`ERR_BADOPCODE`).
    fn asm_instruction(&mut self) {
        if self.kw(0, Keyword::Local, "local") {
            self.local_decl(true);
            return;
        }
        if !self
            .current()
            .is_some_and(|t| qc_preproc::is_opcode_name(&t.text))
        {
            let message = format!("Bad op code name {}", self.found());
            self.error(Code::E0511, message);
            self.recover(true);
            return;
        }
        while !self.at(K::Semicolon) && !self.at(K::RBrace) && self.current().is_some() {
            self.bump();
        }
        self.end_statement();
    }

    /// Whether a C label starts here: a name with its `:` right after it (FTEF-18.23), as
    /// `if (pr_token_type == tt_name && pr_file_p[0] == ':' && pr_file_p[1] != ':')` reads.
    pub(super) fn at_c_label(&self) -> bool {
        let (Some(name), Some(colon)) = (self.current(), self.nth(1)) else {
            return false;
        };
        name.kind == K::Name && colon.kind == K::Colon && colon.span.start == name.span.end
    }

    /// `name:` or `:name` (FTEF-18.23). A label read since the end of the last function is
    /// "Duplicate label NAME", a warning, and is not added again; a duplicate `name:` leaves its
    /// `:` (`QCC_PR_Lex(); return;`), which starts a `:name` label. A `:` before no name is
    /// `invalid label name "TOKEN"`.
    pub(super) fn label_stmt(&mut self) {
        self.start_node(K::LabelStmt);
        let frikqcc = self.at(K::Colon);
        if frikqcc {
            self.bump();
        }
        let Some((name, span)) = self
            .current()
            .filter(|t| t.kind == K::Name)
            .map(|t| (t.text.clone(), t.span))
        else {
            let token = self.current().map(|t| t.text.clone()).unwrap_or_default();
            self.error(Code::E0201, format!("invalid label name \"{token}\""));
            self.recover(true);
            self.finish_node();
            return;
        };
        self.name();
        if self.labels.contains(&name) {
            self.warning_at(Code::W0312, format!("Duplicate label {name}"), span);
        } else {
            self.labels.push(name);
            if !frikqcc {
                self.bump();
            }
        }
        self.finish_node();
    }

    /// `goto name;` (FTEF-18.23), whose label is looked for at the end of the function
    /// (FTEF-18.24).
    pub(super) fn goto_stmt(&mut self) {
        self.start_node(K::GotoStmt);
        self.bump_as(K::GotoKw);
        if let Some(tok) = self.current().filter(|t| t.kind == K::Name) {
            self.gotos.push((tok.text.clone(), tok.span));
        }
        self.name();
        self.end_statement();
        self.finish_node();
    }

    /// Whether `until` starts here, with its keyword (FTEF-18.31, FTEF-20.5): by default only
    /// `__until`, the keyword being on only for the Hexen II targets, so `until (e)` is a call.
    pub(super) fn at_until(&self) -> bool {
        self.kw(0, Keyword::Until, "until")
    }

    /// A name, and `::` and a member name after it (FTEF-16.4).
    pub(super) fn scoped_name(&mut self, long: bool) {
        self.start_node(K::NameRef);
        if long {
            self.error(Code::E0202, "name too long");
        }
        self.bump();
        while self.at(K::ColonColon) && self.nth(1).is_some_and(|t| t.kind == K::Name) {
            self.bump();
            if self.current().is_some_and(|t| self.too_long(&t.text)) {
                self.error(Code::E0202, "name too long");
            }
            self.bump();
        }
    }
}
