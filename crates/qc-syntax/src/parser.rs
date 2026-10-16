// SPDX-License-Identifier: GPL-3.0-or-later

//! The recursive-descent parser.
//!
//! The grammar is id's: `PR_ParseDefs`, `PR_ParseType`, `PR_ParseImmediateStatements`,
//! `PR_ParseState`, `PR_ParseStatement`, `PR_Expression`, `PR_Term` and
//! `PR_ParseFunctionCall` in `qcc/pr_comp.c` and `qcc/pr_lex.c`, as `docs/spec/language.md`
//! states it (QC-10, QC-11, QC-13, QC-14, QC-15). The parser is written from the spec, not
//! ported: it builds a tree instead of emitting statements, and where `qcc` gives up on a
//! declaration after an error, it recovers locally and keeps going (QCD-1.4).
//!
//! Recovery rests on two rules that never change how a correct program parses:
//!
//! - A declaration-shaped line at column 0 (a type word followed by a name, a parameter list
//!   and a name, or a `.`) cannot start a statement, so inside a function body it is taken as
//!   the start of the next declaration and closes the body.
//! - After an error, tokens are skipped to the end of the statement or declaration (`;` at
//!   brace depth 0), to a `}` that closes the block, or to such a declaration line.
//!
//! Only the first error of each statement or declaration is reported, as `qcc` reports one
//! before abandoning it (QC-24.2); the rest would be consequences of it.

use qc_lexer::{Code, Diagnostic, Span};

use crate::input::{Input, Piece, Tok};
use crate::kind::SyntaxKind as K;
use crate::tree::{Builder, Checkpoint, GreenNode};

/// How deep statements, types and expressions may nest (QC-27.15).
///
/// `QC_MAX_NESTING` in `docs/spec/language.md`.
pub const QC_MAX_NESTING: usize = 256;

/// Most parameters a function type may have.
///
/// `LIMIT_MAX_PARMS` in `docs/spec/limits.md`, `MAX_PARMS` in `qcc/qcc.h`. More is an error
/// (QC-27.7).
pub const LIMIT_MAX_PARMS: usize = 8;

/// `qcc`'s name buffer, `MAX_NAME` in `qcc/qcc.h`: a name of `LIMIT_QCC_MAX_NAME - 1` bytes or
/// more is "name too long" (QC-3.3, LIMIT-7.2).
pub const LIMIT_QCC_MAX_NAME: usize = 64;

/// `TOP_PRIORITY` in `qcc/pr_comp.c`: the loosest binary operator priority.
const TOP_PRIORITY: u8 = 6;

/// `NOT_PRIORITY` in `qcc/pr_comp.c`: the priority of the operand of `!` (QC-15.6).
const NOT_PRIORITY: u8 = 4;

/// The priority of `=`, the only right-associative operator.
const ASSIGN_PRIORITY: u8 = 5;

/// How far the declaration-line test looks for the `)` of a parameter list.
const LOOKAHEAD: usize = 256;

/// The type words, `PR_ParseType`'s comparisons (QC-10.1).
fn type_keyword(word: &str) -> Option<K> {
    match word {
        "void" => Some(K::VoidKw),
        "float" => Some(K::FloatKw),
        "vector" => Some(K::VectorKw),
        "string" => Some(K::StringKw),
        "entity" => Some(K::EntityKw),
        _ => None,
    }
}

/// The words that start a statement (QC-14.1).
const STATEMENT_WORDS: [&str; 5] = ["return", "while", "do", "local", "if"];

/// The priority of a binary operator (QC-15.1), from `pr_opcodes`.
const fn binary_priority(kind: K) -> Option<u8> {
    match kind {
        K::Star | K::Slash | K::Amp | K::Pipe => Some(2),
        K::Plus | K::Minus => Some(3),
        K::EqEq | K::NotEq | K::LessEq | K::GreaterEq | K::Less | K::Greater => Some(4),
        K::Eq => Some(ASSIGN_PRIORITY),
        K::AndAnd | K::OrOr => Some(TOP_PRIORITY),
        _ => None,
    }
}

/// What a parsed type is, for the checks on declarations.
#[derive(Clone, Copy, Debug)]
struct TypeShape {
    function: bool,
    field: bool,
}

/// What a parsed expression is, for the checks on assignments and initialisers.
#[derive(Clone, Copy, Debug)]
struct Expr {
    kind: K,
    assignable: bool,
    span: Span,
}

pub(crate) struct Parser<'a> {
    input: Input<'a>,
    pos: usize,
    emitted: usize,
    last_end: usize,
    builder: Builder,
    errors: Vec<Diagnostic>,
    muted: bool,
    depth: usize,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(input: Input<'a>) -> Self {
        Self {
            input,
            pos: 0,
            emitted: 0,
            last_end: 0,
            builder: Builder::default(),
            errors: Vec::new(),
            muted: false,
            depth: 0,
        }
    }

    /// The tree and every diagnostic, lexical ones included, in source order.
    pub(crate) fn finish(self) -> (GreenNode, Vec<Diagnostic>) {
        let mut diagnostics = self.input.diagnostics;
        diagnostics.extend(self.errors);
        diagnostics.sort_by_key(|d| d.span.start);
        (self.builder.finish(), diagnostics)
    }

    // ---- tokens ----

    fn nth(&self, n: usize) -> Option<&Tok> {
        self.input.tokens.get(self.pos.checked_add(n)?)
    }

    fn current(&self) -> Option<&Tok> {
        self.nth(0)
    }

    fn at(&self, kind: K) -> bool {
        self.current().is_some_and(|t| t.kind == kind)
    }

    fn at_word(&self, word: &str) -> bool {
        self.current()
            .is_some_and(|t| t.kind == K::Name && t.text == word)
    }

    fn current_span(&self) -> Span {
        self.current().map_or_else(
            || Span::new(self.input.source.len(), self.input.source.len()),
            |t| t.span,
        )
    }

    /// The current token as `qcc` prints it in "found X"; nothing at the end of the file.
    fn found(&self) -> String {
        self.current().map(|t| t.text.clone()).unwrap_or_default()
    }

    /// Whether the token `n` ahead starts a declaration: a `.`, or a type word followed by a
    /// name or by a parameter list and a name. None of these can start a statement.
    fn looks_like_declaration(&self, n: usize) -> bool {
        let Some(tok) = self.nth(n) else {
            return false;
        };
        if tok.kind == K::Dot {
            return true;
        }
        if tok.kind != K::Name || type_keyword(&tok.text).is_none() {
            return false;
        }
        let next = n.saturating_add(1);
        match self.nth(next).map(|t| t.kind) {
            Some(K::Name) => true,
            Some(K::LParen) => {
                let mut depth = 0_usize;
                for ahead in next..next.saturating_add(LOOKAHEAD) {
                    match self.nth(ahead).map(|t| t.kind) {
                        Some(K::LParen) => depth = depth.saturating_add(1),
                        Some(K::RParen) => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                let after = self.nth(ahead.saturating_add(1)).map(|t| t.kind);
                                return after == Some(K::Name);
                            }
                        }
                        None => return false,
                        Some(_) => {}
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Whether the current token starts a declaration at column 0, where recovery resumes.
    fn at_declaration_line(&self) -> bool {
        self.current().is_some_and(|t| t.column0) && self.looks_like_declaration(0)
    }

    /// Whether the current token is a statement keyword first on its line.
    fn at_statement_line(&self) -> bool {
        self.current().is_some_and(|t| {
            t.line_start && t.kind == K::Name && STATEMENT_WORDS.contains(&t.text.as_str())
        })
    }

    // ---- tree building ----

    fn emit(&mut self, piece: Piece) {
        match piece {
            Piece::Leaf(kind, span) => {
                let text = self.input.source.get(span.range()).unwrap_or_default();
                self.builder.token(kind, text);
            }
            Piece::OpenDirective => self.builder.start_node(K::Directive),
            Piece::CloseDirective => self.builder.finish_node(),
            Piece::Token(index) => {
                if let Some(tok) = self.input.tokens.get(index) {
                    let text = self.input.source.get(tok.span.range()).unwrap_or_default();
                    self.builder.token(tok.kind, text);
                }
            }
        }
    }

    /// Adds the trivia before the current token to the current node.
    fn flush(&mut self) {
        let until = self.current().map_or(self.input.pieces.len(), |t| t.piece);
        while self.emitted < until {
            if let Some(&piece) = self.input.pieces.get(self.emitted) {
                self.emit(piece);
            }
            self.emitted = self.emitted.saturating_add(1);
        }
    }

    fn bump_as(&mut self, kind: K) {
        let Some(tok) = self.current() else {
            return;
        };
        let (span, piece) = (tok.span, tok.piece);
        self.flush();
        let text = self.input.source.get(span.range()).unwrap_or_default();
        self.builder.token(kind, text);
        self.emitted = piece.saturating_add(1);
        self.pos = self.pos.saturating_add(1);
        self.last_end = span.end;
    }

    fn bump(&mut self) {
        if let Some(kind) = self.current().map(|t| t.kind) {
            self.bump_as(kind);
        }
    }

    fn start_node(&mut self, kind: K) {
        self.flush();
        self.builder.start_node(kind);
    }

    fn checkpoint(&mut self) -> Checkpoint {
        self.flush();
        self.builder.checkpoint()
    }

    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    /// Puts the current token, if any, in an error node.
    fn bump_error(&mut self) {
        if self.current().is_some() {
            self.start_node(K::Error);
            self.bump();
            self.finish_node();
        }
    }

    // ---- diagnostics ----

    fn error_at(&mut self, code: Code, message: impl Into<String>, span: Span) {
        if self.muted {
            return;
        }
        self.muted = true;
        self.errors.push(Diagnostic {
            code,
            message: message.into(),
            span,
        });
    }

    fn error(&mut self, code: Code, message: impl Into<String>) {
        let span = self.current_span();
        self.error_at(code, message, span);
    }

    /// `PR_Expect`'s error: "expected X, found Y" (QCD-3.4).
    fn expected(&mut self, what: &str) {
        let message = format!("expected {what}, found {}", self.found());
        self.error(Code::E0204, message);
    }

    fn expect(&mut self, kind: K, what: &str) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            self.expected(what);
            false
        }
    }

    /// Enters one level of nesting; false, with the error reported, past the limit
    /// (QC-27.15). Each call is paired with a [`Parser::leave`].
    fn enter(&mut self) -> bool {
        self.depth = self.depth.saturating_add(1);
        if self.depth > QC_MAX_NESTING {
            self.error(Code::E0509, "nesting too deep");
            false
        } else {
            true
        }
    }

    fn leave(&mut self, levels: usize) {
        self.depth = self.depth.saturating_sub(levels);
    }

    // ---- recovery ----

    /// Skips tokens into an error node until the end of a statement or declaration: a `;` at
    /// brace depth 0 (taken), or, without taking it, a `}` closing the block when `in_block`, a
    /// statement keyword starting a line when `in_block`, or a declaration line.
    fn recover(&mut self, in_block: bool) {
        self.skip(in_block, false);
    }

    /// [`Parser::recover`], taking at least one token when `force` is set.
    fn skip(&mut self, in_block: bool, force: bool) {
        let mut depth = 0_usize;
        let mut open = false;
        while let Some(kind) = self.current().map(|t| t.kind) {
            let first = force && !open;
            let resume =
                self.at_declaration_line() || (in_block && depth == 0 && self.at_statement_line());
            if resume && !first {
                break;
            }
            match kind {
                K::LBrace => depth = depth.saturating_add(1),
                K::RBrace if depth == 0 && in_block => break,
                K::RBrace => depth = depth.saturating_sub(1),
                _ => {}
            }
            if !open {
                self.start_node(K::Error);
                open = true;
            }
            self.bump();
            if kind == K::Semicolon && depth == 0 {
                break;
            }
        }
        if open {
            self.finish_node();
        }
    }

    // ---- declarations ----

    /// A whole file: declarations, and the trivia and directives around them (QC-11.1).
    pub(crate) fn source_file(&mut self) {
        self.builder.start_node(K::SourceFile);
        while let Some(tok) = self.current() {
            let declaration =
                tok.kind == K::Dot || (tok.kind == K::Name && type_keyword(&tok.text).is_some());
            self.muted = false;
            let before = self.pos;
            if declaration {
                self.declaration();
            } else {
                let message = format!("\"{}\" is not a type", self.found());
                self.error(Code::E0203, message);
                self.skip(false, true);
            }
            if self.pos == before {
                self.bump_error();
            }
        }
        self.flush();
        self.finish_node();
    }

    /// `type name [= init] {, name [= init]} ;` (QC-11.1, QC-11.2).
    fn declaration(&mut self) {
        self.start_node(K::Declaration);
        match self.type_() {
            Some(ty) => self.declarators(ty, false),
            None => self.recover(false),
        }
        self.finish_node();
    }

    fn declarators(&mut self, ty: TypeShape, local: bool) {
        loop {
            self.declarator(ty);
            if !self.at(K::Comma) {
                break;
            }
            self.bump();
        }
        if !self.expect(K::Semicolon, ";") {
            self.recover(local);
        }
    }

    fn declarator(&mut self, ty: TypeShape) {
        self.start_node(K::Declarator);
        self.name();
        if self.at(K::Eq) {
            self.bump();
            self.initializer(ty);
        } else if ty.function && (self.at(K::LBrace) || self.at(K::LBracket)) {
            // `PR_ParseDefs` finds no `=` or `,` and expects `;`; the body is kept.
            self.expected(";");
            self.function_body();
        }
        self.finish_node();
    }

    /// `PR_ParseName`: a name, at most `LIMIT_QCC_MAX_NAME - 2` bytes (QC-3.3).
    fn name(&mut self) -> bool {
        let Some(tok) = self.current() else {
            self.error(Code::E0201, "not a name");
            return false;
        };
        if tok.kind != K::Name {
            self.error(Code::E0201, "not a name");
            return false;
        }
        if tok.text.len() >= LIMIT_QCC_MAX_NAME.saturating_sub(1) {
            self.error(Code::E0202, "name too long");
        }
        self.bump();
        true
    }

    fn initializer(&mut self, ty: TypeShape) {
        if ty.function {
            if self.at(K::Hash) {
                self.builtin();
            } else {
                self.function_body();
            }
            return;
        }
        let start = self.current_span().start;
        if self.at(K::LBrace) || self.at(K::LBracket) {
            self.function_body();
        } else if self.at(K::Hash) {
            self.builtin();
        } else {
            match self.expression() {
                Some(expr) if expr.kind == K::Literal => return,
                Some(_) => {}
                None => return,
            }
        }
        let span = Span::new(start, self.last_end.max(start));
        self.error_at(Code::E0506, "initialiser is not a constant", span);
    }

    /// `#number` (QC-13.3, QC-27.9).
    fn builtin(&mut self) {
        self.start_node(K::Builtin);
        self.bump();
        let number = self.current().map(|t| (t.kind, t.value));
        match number {
            Some((K::FloatLit, Some(value))) => {
                #[expect(
                    clippy::float_cmp,
                    reason = "spec QC-13.3: `qcc` compares the value with its int conversion"
                )]
                let integral = value.trunc() == value;
                if !integral {
                    self.error(Code::E0209, "Bad builtin immediate");
                } else if value <= 0.0 {
                    self.error(Code::E0507, "builtin number must be positive");
                }
                self.bump();
            }
            Some((K::FrameRef, _)) => self.bump(),
            _ => self.error(Code::E0209, "Bad builtin immediate"),
        }
        self.finish_node();
    }

    /// `[frame, next] { ... }` or `{ ... }` (QC-13.1, QC-13.6).
    ///
    /// Without a `{` or `[`, the statements are still read as the body when they start on a
    /// new line, as they do when only the `{` is missing; anything else on the line is left
    /// for the declaration's recovery.
    fn function_body(&mut self) {
        let body = self.at(K::LBrace)
            || self.at(K::LBracket)
            || self.current().is_some_and(|t| t.line_start);
        if !body {
            self.expected("{");
            return;
        }
        self.start_node(K::FunctionBody);
        if self.at(K::LBracket) {
            self.state();
        }
        self.block(true);
        self.finish_node();
    }

    /// `[frame, next]` (QC-13.6).
    fn state(&mut self) {
        self.start_node(K::State);
        self.bump();
        if self.at(K::FloatLit) || self.at(K::FrameRef) {
            self.start_node(K::Literal);
            self.bump();
            self.finish_node();
        } else {
            self.error(Code::E0210, "state frame must be a number");
            self.expression();
        }
        self.expect(K::Comma, ",");
        self.name();
        self.expect(K::RBracket, "]");
        self.finish_node();
    }

    // ---- types ----

    /// `PR_ParseType`: `.T`, a simple type, or a simple type and a parameter list (QC-10).
    fn type_(&mut self) -> Option<TypeShape> {
        if !self.enter() {
            self.bump_error();
            self.leave(1);
            return None;
        }
        let shape = self.type_inner();
        self.leave(1);
        shape
    }

    fn type_inner(&mut self) -> Option<TypeShape> {
        if self.at(K::Dot) {
            self.start_node(K::FieldType);
            self.bump();
            let inner = self.type_();
            self.finish_node();
            return inner.map(|_| TypeShape {
                function: false,
                field: true,
            });
        }
        let keyword = self
            .current()
            .filter(|t| t.kind == K::Name)
            .and_then(|t| type_keyword(&t.text));
        let Some(keyword) = keyword else {
            let message = format!("\"{}\" is not a type", self.found());
            self.error(Code::E0203, message);
            return None;
        };
        let checkpoint = self.checkpoint();
        self.start_node(K::SimpleType);
        self.bump_as(keyword);
        self.finish_node();
        if !self.at(K::LParen) {
            return Some(TypeShape {
                function: false,
                field: false,
            });
        }
        self.builder.start_node_at(checkpoint, K::FunctionType);
        self.param_list();
        self.finish_node();
        Some(TypeShape {
            function: true,
            field: false,
        })
    }

    /// `( )`, `( ... )` or `( type name {, type name} )` (QC-10.3, QC-10.4, QC-27.7).
    fn param_list(&mut self) {
        self.start_node(K::ParamList);
        let start = self.current_span().start;
        self.bump();
        if self.at(K::RParen) {
            self.bump();
            self.finish_node();
            return;
        }
        if self.at(K::Ellipsis) {
            self.bump();
        } else {
            let mut count = 0_usize;
            loop {
                self.start_node(K::Param);
                if self.type_().is_some() {
                    self.name();
                } else {
                    self.skip_param();
                }
                self.finish_node();
                count = count.saturating_add(1);
                if !self.at(K::Comma) {
                    break;
                }
                self.bump();
            }
            let closed = self.expect(K::RParen, ")");
            if closed && count > LIMIT_MAX_PARMS {
                let span = Span::new(start, self.last_end);
                self.error_at(
                    Code::E0505,
                    "more than eight parameters in a function type",
                    span,
                );
            }
            self.finish_node();
            return;
        }
        self.expect(K::RParen, ")");
        self.finish_node();
    }

    /// Skips a parameter that has no type, up to the next `,` or `)`.
    fn skip_param(&mut self) {
        let mut open = false;
        while let Some(kind) = self.current().map(|t| t.kind) {
            let stop = matches!(
                kind,
                K::Comma | K::RParen | K::Semicolon | K::Eq | K::LBrace | K::RBrace
            );
            if stop || self.at_declaration_line() {
                break;
            }
            if !open {
                self.start_node(K::Error);
                open = true;
            }
            self.bump();
        }
        if open {
            self.finish_node();
        }
    }

    // ---- statements ----

    /// `{ statements }`. A function's body may be empty; a nested block may not (QC-14.2).
    fn block(&mut self, function: bool) {
        self.start_node(K::Block);
        self.expect(K::LBrace, "{");
        let mut count = 0_usize;
        loop {
            let Some(kind) = self.current().map(|t| t.kind) else {
                // `PR_ParseStatement` reads the end of the file as an expression.
                self.error(Code::E0201, "not a name");
                break;
            };
            if kind == K::RBrace {
                if !function && count == 0 {
                    self.error(Code::E0201, "not a name");
                }
                self.bump();
                break;
            }
            if self.at_declaration_line() {
                self.expected("}");
                break;
            }
            self.muted = false;
            let before = self.pos;
            self.statement();
            if self.pos == before {
                self.bump_error();
            }
            count = count.saturating_add(1);
        }
        self.finish_node();
    }

    /// `PR_ParseStatement` (QC-14.1).
    fn statement(&mut self) {
        if self.enter() {
            self.statement_inner();
        } else {
            self.bump_error();
        }
        self.leave(1);
    }

    fn statement_inner(&mut self) {
        let Some(tok) = self.current() else {
            self.error(Code::E0201, "not a name");
            return;
        };
        match tok.kind {
            K::LBrace => self.block(false),
            K::Dot => self.local_decl(false),
            K::Name => match tok.text.as_str() {
                "return" => self.return_stmt(),
                "while" => self.while_stmt(),
                "do" => self.do_stmt(),
                "local" => self.local_decl(true),
                "if" => self.if_stmt(),
                _ if self.looks_like_declaration(0) => self.local_decl(false),
                _ => self.expr_stmt(),
            },
            _ => self.expr_stmt(),
        }
    }

    /// `local type name [= init] {, ...} ;` (QC-11.10, QC-11.9). Without `local`, the
    /// declaration is an error; it is parsed the same way so the tree keeps its shape.
    fn local_decl(&mut self, keyword: bool) {
        self.start_node(K::LocalDecl);
        if keyword {
            self.bump_as(K::LocalKw);
        } else if self.at(K::Dot) {
            self.error(Code::E0201, "not a name");
        } else {
            let found = self.nth(1).map(|t| (t.text.clone(), t.span));
            if let Some((text, span)) = found {
                self.error_at(Code::E0204, format!("expected ;, found {text}"), span);
            }
        }
        let start = self.current_span().start;
        match self.type_() {
            Some(ty) => {
                if ty.field || ty.function {
                    let span = Span::new(start, self.last_end);
                    self.error_at(Code::E0208, "Fields and functions must be global", span);
                }
                self.declarators(ty, true);
            }
            None => self.recover(true),
        }
        self.finish_node();
    }

    fn return_stmt(&mut self) {
        self.start_node(K::ReturnStmt);
        self.bump_as(K::ReturnKw);
        if self.at(K::Semicolon) {
            self.bump();
        } else {
            self.expression();
            self.end_statement();
        }
        self.finish_node();
    }

    fn while_stmt(&mut self) {
        self.start_node(K::WhileStmt);
        self.bump_as(K::WhileKw);
        self.condition();
        self.statement();
        self.finish_node();
    }

    fn do_stmt(&mut self) {
        self.start_node(K::DoStmt);
        self.bump_as(K::DoKw);
        self.statement();
        if self.at_word("while") {
            self.bump_as(K::WhileKw);
        } else {
            self.expected("while");
        }
        self.condition();
        self.end_statement();
        self.finish_node();
    }

    fn if_stmt(&mut self) {
        self.start_node(K::IfStmt);
        self.bump_as(K::IfKw);
        self.condition();
        self.statement();
        if self.at_word("else") {
            self.bump_as(K::ElseKw);
            self.statement();
        }
        self.finish_node();
    }

    /// `( expression )`; the parentheses are required (QC-14.7).
    fn condition(&mut self) {
        self.expect(K::LParen, "(");
        self.expression();
        self.expect(K::RParen, ")");
    }

    fn expr_stmt(&mut self) {
        self.start_node(K::ExprStmt);
        self.expression();
        self.end_statement();
        self.finish_node();
    }

    fn end_statement(&mut self) {
        if !self.expect(K::Semicolon, ";") {
            self.recover(true);
        }
    }

    // ---- expressions ----

    /// `PR_Expression (TOP_PRIORITY)`.
    fn expression(&mut self) -> Option<Expr> {
        self.expr(TOP_PRIORITY)
    }

    /// An expression whose binary operators have priority `max` or tighter (QC-15.1).
    ///
    /// Precedence climbing, equivalent to `PR_Expression`'s recursion over priorities: a
    /// left-associative operator of priority `p` takes a right operand of priority `p - 1`,
    /// `=` one of its own priority.
    fn expr(&mut self, max: u8) -> Option<Expr> {
        if !self.enter() {
            self.bump_error();
            self.leave(1);
            return None;
        }
        let expr = self.expr_inner(max);
        self.leave(1);
        expr
    }

    fn expr_inner(&mut self, max: u8) -> Option<Expr> {
        let checkpoint = self.checkpoint();
        let mut lhs = self.postfix(checkpoint)?;
        let mut levels = 0_usize;
        while let Some(priority) = self.current().and_then(|t| binary_priority(t.kind)) {
            if priority > max {
                break;
            }
            levels = levels.saturating_add(1);
            if !self.enter() {
                self.bump_error();
                break;
            }
            if priority == ASSIGN_PRIORITY && !lhs.assignable {
                self.error_at(Code::E0508, "cannot assign to this expression", lhs.span);
            }
            self.builder.start_node_at(checkpoint, K::BinaryExpr);
            self.bump();
            let rhs_max = if priority == ASSIGN_PRIORITY {
                priority
            } else {
                priority.saturating_sub(1)
            };
            let rhs = self.expr(rhs_max);
            self.finish_node();
            lhs = Expr {
                kind: K::BinaryExpr,
                assignable: false,
                span: Span::new(lhs.span.start, self.last_end),
            };
            if rhs.is_none() {
                break;
            }
        }
        self.leave(levels);
        Some(lhs)
    }

    /// A term followed by field accesses, or by one call, which ends it (QC-15.8).
    fn postfix(&mut self, checkpoint: Checkpoint) -> Option<Expr> {
        let mut expr = self.term()?;
        let mut levels = 0_usize;
        loop {
            let call = self.at(K::LParen);
            if !call && !self.at(K::Dot) {
                break;
            }
            levels = levels.saturating_add(1);
            if !self.enter() {
                self.bump_error();
                break;
            }
            let start = expr.span.start;
            if call {
                self.builder.start_node_at(checkpoint, K::CallExpr);
                self.arg_list();
                self.finish_node();
                expr = Expr {
                    kind: K::CallExpr,
                    assignable: false,
                    span: Span::new(start, self.last_end),
                };
                break;
            }
            self.builder.start_node_at(checkpoint, K::FieldExpr);
            self.bump();
            let field = self.term();
            self.finish_node();
            expr = Expr {
                kind: K::FieldExpr,
                assignable: true,
                span: Span::new(start, self.last_end),
            };
            if field.is_none() {
                break;
            }
        }
        self.leave(levels);
        Some(expr)
    }

    /// `( args )` (QC-18.1).
    fn arg_list(&mut self) {
        self.start_node(K::ArgList);
        self.bump();
        if self.at(K::RParen) {
            self.bump();
        } else {
            loop {
                self.expression();
                if !self.at(K::Comma) {
                    break;
                }
                self.bump();
            }
            self.expect(K::RParen, ")");
        }
        self.finish_node();
    }

    /// `PR_Term`: `!` expression, `( expression )`, a literal, a frame macro reference or a
    /// name (QC-15.7). Anything else is "not a name" (QC-4.5, QC-14.2).
    fn term(&mut self) -> Option<Expr> {
        let Some(tok) = self.current() else {
            self.error(Code::E0201, "not a name");
            return None;
        };
        let (kind, start, long) = (
            tok.kind,
            tok.span.start,
            tok.text.len() >= LIMIT_QCC_MAX_NAME.saturating_sub(1),
        );
        let (node, assignable) = match kind {
            K::Not => {
                self.start_node(K::NotExpr);
                self.bump();
                self.expr(NOT_PRIORITY);
                (K::NotExpr, false)
            }
            K::LParen => {
                self.start_node(K::ParenExpr);
                self.bump();
                let inner = self.expression();
                self.expect(K::RParen, ")");
                (K::ParenExpr, inner.is_some_and(|e| e.assignable))
            }
            K::FloatLit | K::VectorLit | K::StringLit | K::FrameRef => {
                self.start_node(K::Literal);
                self.bump();
                (K::Literal, false)
            }
            K::Name => {
                self.start_node(K::NameRef);
                if long {
                    self.error(Code::E0202, "name too long");
                }
                self.bump();
                (K::NameRef, true)
            }
            _ => {
                self.error(Code::E0201, "not a name");
                let stop = matches!(
                    kind,
                    K::Semicolon | K::RParen | K::RBracket | K::RBrace | K::Comma | K::LBrace
                );
                if !stop && !self.at_declaration_line() {
                    self.bump_error();
                }
                return None;
            }
        };
        self.finish_node();
        Some(Expr {
            kind: node,
            assignable,
            span: Span::new(start, self.last_end),
        })
    }
}
