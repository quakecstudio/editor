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

use qc_lexer::{Code, Span};

use super::{Parser, TypeShape, UNARY_PRIORITY, is_reserved_word};
use crate::kind::SyntaxKind as K;

/// Most parameters an FTEQCC function type may have: `MAX_PARMS+MAX_EXTRA_PARMS` in
/// `QCC_PR_ParseFunctionType` ("Too many parameters. Sorry. (limit is %i)"), 8 and 128
/// (FTEF-15.4).
pub(super) const FTE_MAX_PARMS: usize = 136;

/// The word after a leading `__`, or the word itself: FTEQCC's `QCC_PR_CheckKeyword` matches
/// `__word` against any keyword, ignoring case, whether or not the keyword is enabled
/// (FTEF-15.1).
fn bare(word: &str) -> (bool, &str) {
    word.strip_prefix("__")
        .map_or((false, word), |rest| (true, rest))
}

/// Whether `word` is a declaration qualifier FTEQCC's `QCC_PR_ParseDefs` reads before a type
/// (FTEF-15.1): one of the enabled keywords, or any of them and the disabled ones after `__`.
pub(super) fn is_decl_qualifier(word: &str) -> bool {
    const ENABLED: [&str; 12] = [
        "extern",
        "static",
        "shared",
        "const",
        "var",
        "nonstatic",
        "noref",
        "nosave",
        "strip",
        "inline",
        "wrap",
        "weak",
    ];
    const PREFIXED: [&str; 7] = [
        "auto",
        "register",
        "unused",
        "used",
        "ignore",
        "accumulate",
        "deprecated",
    ];
    let (prefixed, rest) = bare(word);
    if prefixed {
        let lower = rest.to_ascii_lowercase();
        ENABLED.contains(&lower.as_str()) || PREFIXED.contains(&lower.as_str())
    } else {
        ENABLED.contains(&word)
    }
}

/// Whether `word` is one of the C scalar type words `QCC_PR_ParseType` combines (FTEF-2.4),
/// or `float`, which joins them (`long double`, `unsigned float`).
fn is_c_type_word(word: &str) -> bool {
    let (prefixed, rest) = bare(word);
    let rest = if prefixed {
        rest.to_ascii_lowercase()
    } else {
        rest.to_owned()
    };
    matches!(
        rest.as_str(),
        "signed"
            | "unsigned"
            | "long"
            | "short"
            | "int"
            | "integer"
            | "char"
            | "double"
            | "register"
            | "volatile"
            | "_Bool"
    )
}

/// The class member qualifiers of `QCC_PR_ParseEntClass` (FTEF-9.2, FTEF-9.3).
fn is_member_qualifier(word: &str) -> bool {
    matches!(
        bare(word).1,
        "nonvirtual" | "virtual" | "static" | "const" | "var" | "ignore" | "strip" | "inline"
    )
}

/// The parameter qualifiers of `QCC_PR_ParseFunctionType` (FTEF-15.3): `optional` and, after
/// `__`, `in`, `out` and `inout`.
fn is_param_qualifier(word: &str) -> bool {
    word == "optional" || matches!(bare(word), (true, "optional" | "in" | "out" | "inout"))
}

impl Parser<'_> {
    /// Whether the token `n` ahead is a name spelling `word`, or `__word` in any case.
    pub(super) fn fte_word(&self, n: usize, word: &str) -> bool {
        self.nth(n).is_some_and(|t| {
            t.kind == K::Name && {
                let (prefixed, rest) = bare(&t.text);
                if prefixed {
                    rest.eq_ignore_ascii_case(word)
                } else {
                    rest == word
                }
            }
        })
    }

    /// Whether the token `n` ahead is a name that can start a type: a vanilla or C type word,
    /// or a name that is not a statement word (a type the program defines, FTEF-7.2).
    pub(super) fn fte_type_name(&self, n: usize) -> bool {
        self.nth(n).is_some_and(|t| {
            t.kind == K::Name
                && !is_reserved_word(&t.text)
                && !is_decl_qualifier(&t.text)
                && !matches!(
                    t.text.as_str(),
                    "class" | "union" | "accessor" | "enumflags" | "not" | "goto"
                )
        })
    }

    /// The qualifiers before a declaration's type (FTEF-15.1), each its own node; a
    /// `__deprecated` takes its optional `("reason")`.
    pub(super) fn fte_qualifiers(&mut self) {
        while let Some(text) = self
            .current()
            .filter(|t| t.kind == K::Name && is_decl_qualifier(&t.text))
            .map(|t| t.text.clone())
        {
            self.start_node(K::Qualifier);
            match text.as_str() {
                "const" => self.bump_as(K::ConstKw),
                "var" => self.bump_as(K::VarKw),
                "nosave" => self.bump_as(K::NosaveKw),
                _ => self.bump(),
            }
            if bare(&text).1.eq_ignore_ascii_case("deprecated") && self.at(K::LParen) {
                self.bump();
                while !self.at(K::RParen) && !self.at(K::Semicolon) && self.current().is_some() {
                    self.bump();
                }
                self.expect(K::RParen, ")");
            }
            self.finish_node();
        }
    }

    /// Whether the current name starts a file-level declaration (FTEF-15.1): a qualifier, a
    /// C type word, a tagged type, or a name that may be a type. `starts_declaration` has
    /// already taken a `.`, a `..` and a vanilla type word.
    pub(super) fn fte_starts_declaration(&self) -> bool {
        let Some(tok) = self.current() else {
            return false;
        };
        match tok.kind {
            K::Name if is_decl_qualifier(&tok.text) || is_c_type_word(&tok.text) => true,
            // `struct tag x;`, `enum name x;`: a tagged type names a declaration's type.
            K::Name if matches!(tok.text.as_str(), "struct" | "union" | "enum" | "enumflags") => {
                true
            }
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
        if is_decl_qualifier(&tok.text) || is_c_type_word(&tok.text) {
            return true;
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
    /// `union` and a tag, the C type words, a vanilla type or a type name, then `*`s, then a
    /// parameter list.
    pub(super) fn fte_type(&mut self) -> Option<TypeShape> {
        if !self.enter() {
            self.bump_error();
            self.leave(1);
            return None;
        }
        let shape = self.fte_type_inner();
        self.leave(1);
        shape
    }

    fn fte_type_inner(&mut self) -> Option<TypeShape> {
        if self.fte_word(0, "const") || self.fte_word(0, "volatile") {
            // `QCC_PR_ParseType` reads and ignores both (FTEF-2.5).
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
            let inner = self.fte_type();
            self.finish_node();
            self.finish_node();
            return inner.map(|_| TypeShape {
                function: false,
                field: true,
            });
        }
        if self.at(K::Dot) {
            self.start_node(K::FieldType);
            self.bump();
            if self.at(K::Star) {
                // `.*float`: a field of pointer to float (FTEF-4.1).
                self.bump();
            }
            let inner = self.fte_type();
            self.finish_node();
            return inner.map(|_| TypeShape {
                function: false,
                field: true,
            });
        }
        let checkpoint = self.checkpoint();
        if !self.fte_base_type() {
            let message = format!("\"{}\" is not a type", self.found());
            self.error(Code::E0203, message);
            return None;
        }
        // FTEF-18.26: each `*` and each parameter list nests the type one level deeper.
        let mut levels = 0_usize;
        while self.at(K::Star) {
            levels = levels.saturating_add(1);
            if !self.enter() {
                break;
            }
            self.builder.start_node_at(checkpoint, K::PointerType);
            self.bump();
            if self.fte_word(0, "const") {
                self.bump();
            }
            self.finish_node();
        }
        let function = self.at_function_params();
        // FTEF-18.15: `QCC_PR_ParseDefs` loops `while(QCC_PR_CheckToken ("("))`, so each
        // further parameter list makes a function returning the type so far.
        while self.at_function_params() {
            levels = levels.saturating_add(1);
            if !self.enter() {
                break;
            }
            self.builder.start_node_at(checkpoint, K::FunctionType);
            self.fte_param_list();
            self.finish_node();
        }
        self.leave(levels);
        Some(TypeShape {
            function,
            field: false,
        })
    }

    /// Whether a type's parameter list starts here: a `(` that opens no `(*name)` (FTEF-15.3).
    fn at_function_params(&self) -> bool {
        self.at(K::LParen) && !self.nth(1).is_some_and(|t| t.kind == K::Star)
    }

    /// The base of a type: a `SimpleType` node holding a vanilla keyword, a run of C type
    /// words, `struct`/`union` and a tag, or a type name. False, consuming nothing, when the
    /// token starts none.
    fn fte_base_type(&mut self) -> bool {
        let Some(tok) = self.current() else {
            return false;
        };
        if tok.kind != K::Name {
            return false;
        }
        let text = tok.text.clone();
        if text == "enum" || text == "enumflags" {
            // FTEF-8.2: an enum written as a type, `enumflags:float{A, B} flags`. FTEF-8.4: it
            // always has its body; an enum is named by its type name alone.
            self.start_node(K::SimpleType);
            self.enum_body();
            self.finish_node();
            return true;
        }
        if (text == "struct" || text == "union")
            && (self.nth(1).is_some_and(|t| t.kind == K::LBrace)
                || self.nth(2).is_some_and(|t| t.kind == K::LBrace))
        {
            // FTEF-6.4: a struct written as a member's type, `struct tag { ... } *field;`.
            self.start_node(K::SimpleType);
            self.struct_type();
            self.finish_node();
            return true;
        }
        if text == "struct" || text == "union" {
            // `struct tag` names a struct by its tag (FTEF-6.1).
            self.start_node(K::SimpleType);
            self.bump_as(if text == "struct" {
                K::StructKw
            } else {
                K::UnionKw
            });
            if self.at(K::Name) {
                self.bump();
            }
            self.finish_node();
            return true;
        }
        if is_c_type_word(&text) {
            self.start_node(K::SimpleType);
            while self.current().is_some_and(|t| {
                t.kind == K::Name && (is_c_type_word(&t.text) || t.text == "float")
            }) {
                self.bump();
            }
            self.finish_node();
            return true;
        }
        if let Some(keyword) = super::type_keyword(&text) {
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
        self.start_node(K::SimpleType);
        self.bump();
        self.finish_node();
        true
    }

    /// `QCC_PR_ParseFunctionType` (FTEF-15.3, FTEF-15.4): `( )`, or parameters separated by
    /// `,`, each with its qualifiers, a type, an optional name or `(*name)(params)`, array
    /// suffixes and a default value, and a `...` that ends the list, optionally with a count
    /// name.
    pub(super) fn fte_param_list(&mut self) {
        self.start_node(K::ParamList);
        let start = self.current_span().start;
        self.bump();
        if self.at(K::RParen) {
            self.bump();
            self.finish_node();
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
            self.fte_param();
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
    }

    /// One parameter of [`Parser::fte_param_list`].
    fn fte_param(&mut self) {
        self.start_node(K::Param);
        while self
            .current()
            .is_some_and(|t| t.kind == K::Name && is_param_qualifier(&t.text))
        {
            self.start_node(K::Qualifier);
            self.bump();
            self.finish_node();
        }
        if self.fte_type().is_none() {
            self.skip_param();
            self.finish_node();
            return;
        }
        if self.at(K::Amp) {
            // `type &name`: an out parameter, C++'s spelling (FTEF-15.3).
            self.bump();
        }
        if self.at(K::Ellipsis) {
            // `type ...`: the variadic arguments are of this type.
            self.bump();
            self.finish_node();
            return;
        }
        if self.at(K::LParen) && self.nth(1).is_some_and(|t| t.kind == K::Star) {
            // `(*name)(params)`: a function-pointer parameter.
            self.bump();
            self.bump();
            self.name();
            if self.at(K::LBracket) {
                self.array_suffix();
            }
            self.expect(K::RParen, ")");
            if self.at(K::LParen) {
                self.fte_param_list();
            }
        } else if self.at(K::Name) {
            self.name();
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
    }

    // ---- file-level constructs ----

    /// `class Name [: Parent] { members } [;]` or the forward `class Name;`
    /// (`QCC_PR_ParseEntClass`, FTEF-9.1).
    pub(super) fn class_decl(&mut self) {
        self.start_node(K::ClassDecl);
        // `static class ...`: `QCC_PR_ParseDefs` reads the qualifiers, then the class as the
        // declaration's type.
        self.fte_qualifiers();
        self.bump_as(K::ClassKw);
        self.name();
        if self.at(K::Colon) {
            self.bump();
            self.name();
        }
        if self.at(K::LBrace) {
            self.bump();
            // FTEF-18.27: a class that lost its `}` ends at the next declaration line.
            while !self.at(K::RBrace) && self.current().is_some() && !self.at_declaration_line() {
                let before = self.pos;
                self.start_item();
                self.class_member();
                if self.pos == before {
                    self.bump_error();
                }
            }
            self.expect(K::RBrace, "}");
        }
        if self.at(K::Semicolon) {
            self.bump();
        }
        self.finish_node();
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
        if !(self.at_word("enum") || self.at_word("enumflags")) {
            return false;
        }
        match self.nth(1).map(|t| t.kind) {
            Some(K::LBrace | K::Colon) => true,
            Some(K::Name) => {
                self.fte_word(1, "class")
                    || self
                        .nth(2)
                        .is_some_and(|t| matches!(t.kind, K::LBrace | K::Colon))
            }
            _ => false,
        }
    }

    /// Whether a class starts here, after any declaration qualifiers (FTEF-9.1).
    pub(super) fn at_class(&self) -> bool {
        let mut n = 0_usize;
        while self
            .nth(n)
            .is_some_and(|t| t.kind == K::Name && is_decl_qualifier(&t.text))
        {
            n = n.saturating_add(1);
        }
        self.fte_word(n, "class")
    }

    /// One member of a class body (FTEF-9.2 to FTEF-9.4, FTEF-10.3): an access or default
    /// label, a `get`/`set` accessor member, or qualifiers and a declaration of fields or
    /// member functions, which may have bodies.
    fn class_member(&mut self) {
        let label = self.current().is_some_and(|t| {
            t.kind == K::Name
                && matches!(
                    bare(&t.text).1,
                    "public" | "private" | "protected" | "virtual" | "nonvirtual"
                )
        }) && self.nth(1).is_some_and(|t| t.kind == K::Colon);
        if label {
            self.start_node(K::AccessLabel);
            self.bump();
            self.bump();
            self.finish_node();
            return;
        }
        if self.at(K::Semicolon) {
            self.bump();
            return;
        }
        let accessor = (self.fte_word(0, "get") || self.fte_word(0, "set"))
            || (self.fte_word(0, "inline") && (self.fte_word(1, "get") || self.fte_word(1, "set")));
        if accessor {
            self.accessor_member();
            return;
        }
        self.start_node(K::Declaration);
        while self
            .current()
            .is_some_and(|t| t.kind == K::Name && is_member_qualifier(&t.text))
        {
            self.start_node(K::Qualifier);
            match self.current().map(|t| t.text.clone()).as_deref() {
                Some("const") => self.bump_as(K::ConstKw),
                Some("var") => self.bump_as(K::VarKw),
                _ => self.bump(),
            }
            self.finish_node();
        }
        match self.fte_type() {
            // `Name(void);`: a constructor with no return type (FTEF-9.4).
            Some(ty) if ty.function && self.at(K::Semicolon) => self.bump(),
            // A member's recovery stops at the `}` that closes the class.
            Some(ty) => self.declarators(ty, true),
            None => self.recover(true),
        }
        self.finish_node();
    }

    /// `struct Name { fields };`, `union Name { ... };` or the forward `struct Name;` at file
    /// level (FTEF-6.1, FTEF-6.2).
    pub(super) fn struct_decl(&mut self) {
        self.start_node(K::StructDecl);
        self.struct_type();
        // FTEF-18.27: a declaration line after a struct that lost its `}` is the next
        // declaration, not a variable of the struct.
        if (self.at(K::Name) || self.at(K::Star)) && !self.at_declaration_line() {
            // FTEF-6.1: `struct { ... } a[4], *p;` declares variables of the struct too.
            let shape = TypeShape {
                function: false,
                field: false,
            };
            self.declarators(shape, false);
        } else if !self.expect(K::Semicolon, ";") {
            self.recover(false);
        }
        self.finish_node();
    }

    /// `accessor Name [: Base] [{ members }] [;]` (FTEF-10.1).
    pub(super) fn accessor_decl(&mut self) {
        self.start_node(K::AccessorDecl);
        self.bump_as(K::AccessorKw);
        self.name();
        if self.at(K::Colon) {
            self.bump();
            self.fte_type();
        }
        if self.at(K::LBrace) {
            self.bump();
            // FTEF-18.27: an accessor that lost its `}` ends at the next declaration line.
            while !self.at(K::RBrace) && self.current().is_some() && !self.at_declaration_line() {
                let before = self.pos;
                self.start_item();
                self.accessor_member();
                if self.at(K::Comma) || self.at(K::Semicolon) {
                    self.bump();
                }
                if self.pos == before {
                    self.bump_error();
                }
            }
            self.expect(K::RBrace, "}");
        }
        if self.at(K::Semicolon) {
            self.bump();
        }
        self.finish_node();
    }

    /// One accessor member (`QCC_PR_ParseAccessorMember`, FTEF-10.2): `[inline] get|set`, a
    /// type, an optional name, an optional `[IndexType [name]]`, and `= function` or
    /// `= { body }`.
    fn accessor_member(&mut self) {
        self.start_node(K::AccessorMember);
        if self.fte_word(0, "inline") {
            self.bump();
        }
        if self.fte_word(0, "get") || self.fte_word(0, "set") {
            self.bump();
        } else {
            self.expected("get or set");
            self.recover(true);
            self.finish_node();
            return;
        }
        if self.fte_type().is_some() {
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
            if self.at(K::Eq) {
                self.bump();
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
        if super::type_keyword(&first.text).is_some() || is_c_type_word(&first.text) {
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
                super::type_keyword(&t.text).is_some()
                    || is_c_type_word(&t.text)
                    || is_param_qualifier(&t.text)
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
            self.expr(UNARY_PRIORITY);
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
                if super::type_keyword(&tok.text).is_some()
                    || is_c_type_word(&tok.text)
                    || matches!(tok.text.as_str(), "struct" | "union" | "enum" | "enumflags")
                {
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

    /// The state header (FTEF-18.3, FTEF-18.4): `[frame, next]` with an expression for each,
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
            // new (QC-13.6); any other expression is a value of the body.
            if self.at(K::Name) && self.nth(1).is_some_and(|t| t.kind == K::RBracket) {
                self.name();
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

    /// `++ first .. last` or `-- first .. last`, after the `[` (FTEF-18.4). Two constant
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

    /// Whether a C label starts here: a name with its `:` right after it (FTEF-18.23), as
    /// `if (pr_token_type == tt_name && pr_file_p[0] == ':' && pr_file_p[1] != ':')` reads.
    pub(super) fn at_c_label(&self) -> bool {
        let (Some(name), Some(colon)) = (self.current(), self.nth(1)) else {
            return false;
        };
        name.kind == K::Name && colon.kind == K::Colon && colon.span.start == name.span.end
    }

    /// `name:` or `:name` (FTEF-18.23). A label read since the end of the last function is
    /// "Duplicate label NAME", a warning, and is not added again.
    pub(super) fn label_stmt(&mut self) {
        self.start_node(K::LabelStmt);
        let frikqcc = self.at(K::Colon);
        if frikqcc {
            self.bump();
        }
        let label = self
            .current()
            .filter(|t| t.kind == K::Name)
            .map(|t| (t.text.clone(), t.span));
        if !self.name() {
            // `invalid label name`: the statement is abandoned.
            self.recover(true);
        } else {
            if !frikqcc {
                self.bump();
            }
            if let Some((name, span)) = label {
                if self.labels.contains(&name) {
                    self.warning_at(Code::W0312, format!("Duplicate label {name}"), span);
                } else {
                    self.labels.push(name);
                }
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

    /// Whether `__until` starts here (FTEF-18.9). `until` itself is a keyword only for the
    /// Hexen II targets, so under this profile it is a name, and `until (e)` a call.
    pub(super) fn at_until(&self) -> bool {
        self.current().is_some_and(|t| {
            t.kind == K::Name
                && matches!(bare(&t.text), (true, rest) if rest.eq_ignore_ascii_case("until"))
        })
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
