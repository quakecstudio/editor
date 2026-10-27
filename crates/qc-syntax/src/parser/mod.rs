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

use qc_lexer::{Code, Diagnostic, Dialect, LexFlag, Span};
use qc_preproc::{CompilerFlags, Enabled, FteFlag, Keyword, KeywordEdit, Keywords, Priorities};

mod fte;

use crate::flags::{Block, Linkage};
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

/// FTEQCC's name buffer, `#define MAX_NAME 256` in FTEQCC's `qcc.h`: under an FTEQCC dialect a
/// name of `FTEQCC_MAX_NAME - 1` bytes or more is "name too long" (FTEF-15.7).
pub const FTEQCC_MAX_NAME: usize = 256;

/// The operator priorities an expression is read with (FTEF-21.4, FTEF-21.5): QuakeC's,
/// `PR_Expression`'s and FTEQCC's without `flag_cpriority`, in id's numbering, one below
/// FTEQCC's, or FTEQCC's C priorities, in its own numbering. Under both a binary operator of
/// priority `p` takes a right operand of priority `p - 1`, an assignment one of its own, and a
/// unary operand is of [`UNARY_PRIORITY`], below every binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Precedence(Priorities);

impl Precedence {
    /// `TOP_PRIORITY`: the loosest binary operator priority, `priority_class[MAX_PRIORITY_CLASSES]`.
    const fn top(self) -> u8 {
        match self.0 {
            Priorities::Qc => 6,
            Priorities::C => 13,
        }
    }

    /// `NOT_PRIORITY`, `priority_class[PC_UNARYNOT]`: the priority of the operand of `!`
    /// (QC-15.6) and `~` (FTE-19.4); under C's, [`UNARY_PRIORITY`].
    const fn not(self) -> u8 {
        match self.0 {
            Priorities::Qc => 4,
            Priorities::C => UNARY_PRIORITY,
        }
    }

    /// `ASSIGN_PRIORITY`, `priority_class[PC_STORE]`: the priority of `=` and the compound
    /// assignments, the right-associative operators.
    const fn assign(self) -> u8 {
        match self.0 {
            Priorities::Qc => 5,
            Priorities::C => 13,
        }
    }

    /// FTEQCC's `TERNARY_PRIORITY`, `priority_class[PC_TERNARY]`: the priority `?:` is read at.
    /// Under QuakeC's it equals the assignment priority and is tighter than `&&` and `||`
    /// (FTE-8.4): `priority_class[PC_TERNARY] = 6; priority_class[PC_STORE] = 6;` and
    /// `priority_class[PC_LOGICAND] = 7;`, one above this numbering, whose `&&` and `||` are 6.
    /// Under C's it is looser than `||` and tighter than the assignments.
    const fn ternary(self) -> u8 {
        match self.0 {
            Priorities::Qc => 5,
            Priorities::C => 12,
        }
    }

    /// The priority of `+` and `-` (QC-15.1), `priority_class[PC_ADDSUB]`.
    const fn add(self) -> u8 {
        ADD_PRIORITY
    }

    /// The priority of a binary operator (QC-15.1), from `pr_opcodes`' priority classes. Under
    /// QuakeC's, FTEQCC's operators at the matching vanilla priority (FTE-6, FTE-7, FTE-19.3):
    /// `QCC_PrioritiseOpcodes` puts shifts, `^`, `&` and `|` in the class of `*` and `/`, with
    /// `*^` and `><` (`PC_MULDIV`), and `<=>` in the class of `==` (`PC_EQUALITY`, FTEF-13.24).
    /// Under C's, each class at its C priority (FTEF-21.5).
    const fn binary(self, kind: K) -> Option<u8> {
        match self.0 {
            Priorities::Qc => match kind {
                K::Star
                | K::Slash
                | K::Amp
                | K::Pipe
                | K::Percent
                | K::Caret
                | K::Shr
                | K::Shl
                | K::StarCaret
                | K::Cross => Some(2),
                K::Plus | K::Minus => Some(ADD_PRIORITY),
                K::EqEq
                | K::NotEq
                | K::LessEq
                | K::GreaterEq
                | K::Less
                | K::Greater
                | K::Spaceship => Some(4),
                // `=` and the FTEQCC compound assignments (FTE-6, FTE-19.3) are
                // right-associative at the assignment priority.
                kind if is_assignment(kind) => Some(5),
                K::AndAnd | K::OrOr => Some(6),
                _ => None,
            },
            // `priority_class[PC_MULDIV] = 2; ... priority_class[PC_STORE] = 13;`
            Priorities::C => match kind {
                K::Star | K::Slash | K::Percent | K::StarCaret | K::Cross => Some(2),
                K::Plus | K::Minus => Some(ADD_PRIORITY),
                K::Shl | K::Shr => Some(4),
                K::Less | K::LessEq | K::Greater | K::GreaterEq => Some(5),
                K::EqEq | K::NotEq | K::Spaceship => Some(6),
                K::Amp => Some(7),
                K::Caret => Some(8),
                K::Pipe => Some(9),
                K::AndAnd => Some(10),
                K::OrOr => Some(11),
                kind if is_assignment(kind) => Some(13),
                _ => None,
            },
        }
    }
}

/// A clause of a `for` header (FTE-2.1), which FTEQCC discards or keeps (FTEF-18.25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clause {
    /// The first: run once, its value discarded.
    Init,
    /// The second: the condition, whose value is kept.
    Condition,
    /// The third: run after each pass, its value discarded.
    Step,
}

/// Where a declaration is, which decides what its declarators define (FTEF-20.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Site {
    /// At file level: each declarator is a global definition.
    File,
    /// In a function body: each declarator is a local definition.
    Body,
    /// In a class body: each declarator is a member, which `QCC_PR_DummyDef` does not name.
    Member,
}

impl Site {
    /// Whether a declaration here is inside a block, where its recovery stops at the block's
    /// `}`.
    const fn in_block(self) -> bool {
        match self {
            Self::File => false,
            Self::Body | Self::Member => true,
        }
    }
}

/// What an accessor member is read in (FTEF-20.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Member {
    /// A class body, whose words `QCC_PR_CheckKeyword(1, ...)` reads.
    Class,
    /// An accessor's body, whose words `QCC_PR_CheckName` reads.
    Accessor,
}

/// The byte a character of a token's text stands for: the text holds each byte as the
/// character of its code (`Tok::text`).
fn latin1_byte(c: char) -> u8 {
    u8::try_from(u32::from(c)).unwrap_or(b'?')
}

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

/// The priority of `+` and `-` (QC-15.1).
const ADD_PRIORITY: u8 = 3;

/// FTEQCC's `UNARY_PRIORITY`: the operand of a prefix `-`, `+` or of a cast is a term with its
/// postfix forms and no binary operator (FTE-19.4, FTE-19.6).
const UNARY_PRIORITY: u8 = 1;

/// Whether a binary operator kind is an assignment (`=` or an FTEQCC compound assignment),
/// so its left operand must be assignable (QC-27.10, FTE-6, FTE-19.3).
const fn is_assignment(kind: K) -> bool {
    matches!(
        kind,
        K::Eq
            | K::PlusEq
            | K::MinusEq
            | K::StarEq
            | K::SlashEq
            | K::AndEq
            | K::OrEq
            | K::AndNotEq
            | K::SetBits
            | K::ClearBits
            | K::CaretEq
            | K::ShlEq
            | K::ShrEq
    )
}

/// What a parsed type is, for the checks on declarations.
#[derive(Clone, Copy, Debug, Default)]
struct TypeShape {
    function: bool,
    field: bool,
    /// Whether `QCC_PR_ParseType` read the type with `qcfuncs` on where a parameter list may
    /// follow it, so a `(` after it was the type's (FTEF-21.7).
    qcfuncs: bool,
}

/// A type the program declared (FTEF-20.14, FTEF-7.5).
#[derive(Clone, Debug)]
struct DeclaredType {
    name: String,
    /// Whether a function's body declared it, which only that body sees.
    local: bool,
    /// Whether it is a function type: a typedef of one, or of C's function pointer.
    function: bool,
}

/// What a parsed expression is, for the checks on assignments and initialisers.
#[derive(Clone, Copy, Debug)]
struct Expr {
    kind: K,
    assignable: bool,
    span: Span,
    tail: Tail,
}

/// What FTEQCC sees in an expression whose value is discarded, past its calls and assignments
/// (FTEF-18.25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tail {
    /// Nothing more.
    Plain,
    /// A postfix `++` or `--` still to be stored, which `QCC_PR_DiscardRef` stores, setting
    /// `qcc_usefulstatement`; a parenthesised one or the last of a sequence is still pending.
    PostInc,
    /// A cast to `void`: the type the discarded expression check of `QCC_PR_DiscardExpression`
    /// lets through (`if (ref->cast->type != ev_void && !qcc_usefulstatement)`).
    Void,
}

pub(crate) struct Parser<'a> {
    input: Input<'a>,
    dialect: Dialect,
    pos: usize,
    emitted: usize,
    last_end: usize,
    builder: Builder,
    errors: Vec<Diagnostic>,
    muted: bool,
    depth: usize,
    /// Whether the nesting limit was passed in the current statement, declaration or member,
    /// so no construct in it opens another level until the next one starts (QC-27.16): the
    /// levels it unwinds through would otherwise go on wrapping what they hold, and the tree
    /// would grow past the limit after all.
    too_deep: bool,
    /// FTEQCC's `qcc_usefulstatement`: whether a call, an assignment or a prefix `++` or `--`
    /// has been read since the flag was last cleared (FTEF-18.25).
    useful: bool,
    /// The labels read since the end of the last function, FTEQCC's `pr_labels`
    /// (FTEF-18.23).
    labels: Vec<String>,
    /// The `goto`s read since the end of the last function, FTEQCC's `pr_gotos`, each with
    /// the span of its label name (FTEF-18.24).
    gotos: Vec<(String, Span)>,
    /// How many function bodies the current token is in, `pr_scope` set when one is.
    bodies: usize,
    /// The names of the parameters of the last parameter list read, each with whether its type
    /// is a function's, which `QCC_PR_ParseImmediateStatements` defines when a body follows
    /// (FTEF-20.4).
    params: Vec<(String, bool)>,
    /// The keywords in force at the current token (FTEF-20.1).
    keywords: Keywords,
    /// The preprocessor's changes to them, each from the byte it holds from, and how many of
    /// them are in [`Self::keywords`].
    edits: Vec<(usize, KeywordEdit)>,
    applied: usize,
    /// The keywords from each byte on, as the parser read them: its starting keywords, each of
    /// [`Self::edits`] and each change a definition made (FTEF-20.4).
    keyword_marks: Vec<(usize, Keywords)>,
    /// The compiler flags from each byte on, as the input was lexed with them (FTEF-21.1).
    flags: Vec<(usize, CompilerFlags)>,
    /// The types the program defined so far, each with whether a function's body defined it,
    /// which only that body sees: `QCC_TypeForName` finds a typedef in the scope it was made
    /// in (FTEF-20.14), and whether it is a function type, whose definitions take a body
    /// (FTEF-7.5).
    types: Vec<DeclaredType>,
    /// Whether the base type last read is a declared type of function type (FTEF-7.5).
    type_function: bool,
    /// The `extern` blocks read, in the order they ended (FTEF-21.17).
    blocks: Vec<Block>,
    /// The bytes, in increasing order, strings start at that FTEQCC's lexer did not read into
    /// the string before them: it joins only the parts written in one text, not a macro's string and the one
    /// beside it (FTEF-21.17).
    seams: Vec<usize>,
    /// How many `extern` blocks with a `{` the current token is in, whose `}` recovery stops at.
    open_blocks: usize,
    /// Whether the expression being read is the operand of `!` or `~`, FTEQCC's
    /// `EXPR_WARN_ABOVE_1` (FTEF-21.6).
    unary_not: bool,
    /// Whether the declarators being read define types, after a `typedef` (FTEF-7.3).
    in_typedef: bool,
    /// Whether a body that lost its `}` ended at a declaration line (FTEF-18.27), so what
    /// follows it in the declaration is the next declaration's, not read as its declarators.
    recovering: bool,
    /// Whether the type last read had a `*` or a parameter list after its base.
    type_derived: bool,
    /// Whether the member just read is a bitfield, which a `,` does not go on with.
    after_bitfield: bool,
    /// Whether the struct member being read is `static` or `virtual`, a global of the struct's
    /// that takes a body or value (FTEF-6.7).
    member_global: bool,
    /// Whether the struct member being read has a function type, whose body a `{` starts.
    member_function: bool,
    /// The class whose members are being read.
    class_name: Option<String>,
    /// For each token, the index of the `}` that pairs with the innermost `{` open before it,
    /// `None` when that `{` never closes: one pass over the tokens, so that a body that
    /// lost its `}` is told apart in constant time at each declaration line (FTEF-18.34).
    brace_close: Vec<Option<usize>>,
}

/// For each token index, the index of the `}` pairing with the innermost `{` still open
/// before it. A `}` with no `{` open closes nothing.
fn brace_closes(tokens: &[Tok]) -> Vec<Option<usize>> {
    let mut open_at: Vec<Option<usize>> = Vec::with_capacity(tokens.len().saturating_add(1));
    let mut close_of: Vec<Option<usize>> = vec![None; tokens.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        open_at.push(stack.last().copied());
        match tok.kind {
            K::LBrace => stack.push(i),
            K::RBrace => {
                if let Some(open) = stack.pop()
                    && let Some(slot) = close_of.get_mut(open)
                {
                    *slot = Some(i);
                }
            }
            _ => {}
        }
    }
    open_at.push(stack.last().copied());
    open_at
        .into_iter()
        .map(|open| open.and_then(|o| close_of.get(o).copied().flatten()))
        .collect()
}

impl<'a> Parser<'a> {
    pub(crate) fn new(
        input: Input<'a>,
        dialect: Dialect,
        edits: &[(usize, KeywordEdit)],
        flags: &[(usize, CompilerFlags)],
        seams: &[usize],
    ) -> Self {
        let brace_close = brace_closes(&input.tokens);
        let mut parser = Self {
            input,
            dialect,
            pos: 0,
            emitted: 0,
            last_end: 0,
            builder: Builder::default(),
            errors: Vec::new(),
            muted: false,
            useful: false,
            labels: Vec::new(),
            gotos: Vec::new(),
            depth: 0,
            too_deep: false,
            bodies: 0,
            params: Vec::new(),
            keywords: Keywords::default(),
            edits: edits.to_vec(),
            applied: 0,
            keyword_marks: Vec::new(),
            flags: flags.to_vec(),
            types: Vec::new(),
            blocks: Vec::new(),
            seams: seams.to_vec(),
            open_blocks: 0,
            unary_not: false,
            in_typedef: false,
            recovering: false,
            type_derived: false,
            after_bitfield: false,
            member_global: false,
            member_function: false,
            type_function: false,
            class_name: None,
            brace_close,
        };
        parser.sync_keywords();
        parser
    }

    /// The `extern` blocks read (FTEF-21.17).
    pub(crate) fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The compiler flags in force at the token `n` ahead (FTEF-21.1), as FTEQCC reads a flag
    /// while that token is the one it looks at.
    fn flags_ahead(&self, n: usize) -> CompilerFlags {
        let at = self
            .nth(n)
            .map_or(self.input.source.len(), |t| t.span.start);
        CompilerFlags::at(&self.flags, at)
    }

    /// Whether `flag` is on at the token `n` ahead (FTEF-21.1).
    fn flag_ahead(&self, n: usize, flag: FteFlag) -> bool {
        self.flags_ahead(n).has(flag)
    }

    /// The spans of the tokens of a preprocessor's text that were lexed with other flags than
    /// `preprocessor`, the flags the preprocessor lexed them with, those of an `extern` block
    /// (FTEF-21.17); their lexical errors and warnings become the parser's.
    pub(crate) fn relexed(&mut self, preprocessor: &[(usize, CompilerFlags)]) -> Vec<Span> {
        let spans: Vec<Span> = self
            .input
            .tokens
            .iter()
            .map(|t| t.span)
            .filter(|span| {
                qc_lexer::LexFlags::at(&self.input.flags, span.start)
                    != CompilerFlags::at(preprocessor, span.start).lex()
            })
            .collect();
        let within = |d: &Diagnostic| {
            spans.iter().any(|span| {
                span.start <= d.span.start
                    && d.span.start < span.end.max(span.start.saturating_add(1))
            })
        };
        let lexical = std::mem::take(&mut self.input.lexical);
        self.errors.extend(lexical.into_iter().filter(within));
        spans
    }

    /// The tree, every diagnostic, lexical ones included, in source order, and the keywords
    /// each token was read with.
    pub(crate) fn finish(mut self) -> (GreenNode, Vec<Diagnostic>, Vec<(usize, Keywords)>) {
        let mut diagnostics = self.input.diagnostics;
        diagnostics.extend(self.errors);
        diagnostics.sort_by_key(|d| d.span.start);
        self.keyword_marks
            .dedup_by(|later, earlier| later.1 == earlier.1);
        (self.builder.finish(), diagnostics, self.keyword_marks)
    }

    // ---- keywords ----

    /// Takes in the preprocessor's keyword changes up to the current token (FTEF-20.1): FTEQCC
    /// reads a `#pragma keyword` as it lexes the token after it, which is the token the parser
    /// looks at next.
    fn sync_keywords(&mut self) {
        let at = self.current_span().start;
        while let Some(&(from, edit)) = self.edits.get(self.applied).filter(|(from, _)| *from <= at)
        {
            self.keywords = edit.apply(self.keywords);
            self.keyword_marks.push((from, self.keywords));
            self.applied = self.applied.saturating_add(1);
        }
    }

    /// The keywords in force at the token `n` ahead: the current ones and the preprocessor's
    /// changes up to it.
    fn keywords_ahead(&self, n: usize) -> Keywords {
        let at = self
            .nth(n)
            .map_or(self.input.source.len(), |t| t.span.start);
        self.edits
            .iter()
            .skip(self.applied)
            .take_while(|(from, _)| *from <= at)
            .fold(self.keywords, |keywords, (_, edit)| edit.apply(keywords))
    }

    /// Whether the token `n` ahead is read with `caseinsens` on, under which FTEQCC searches its
    /// name tables in any case (`Hash_GetInsensitive`, FTEF-19.33). Only a full dialect has it.
    fn any_case(&self, n: usize) -> bool {
        self.fteqcc() && self.keywords_ahead(n).case_insensitive()
    }

    /// The vanilla type word the token `n` ahead names, as `QCC_TypeForName` finds it (QC-10.1,
    /// FTEF-19.33): `void`, `float`, `vector`, `string` or `entity`, in any case under
    /// `caseinsens`.
    fn type_word(&self, n: usize) -> Option<K> {
        let tok = self.nth(n).filter(|t| t.kind == K::Name)?;
        type_keyword(&tok.text).or_else(|| {
            if self.any_case(n) {
                type_keyword(&tok.text.to_ascii_lowercase())
            } else {
                None
            }
        })
    }

    /// `QCC_PR_CheckKeyword(enabled, word)` of the token `n` ahead (FTEF-20.2): a name that is
    /// the keyword with the keywords in force at it. Under the vanilla dialect, which has no
    /// keyword flags, a name that is `word` exactly.
    fn checks(&self, n: usize, enabled: Enabled, word: &str) -> bool {
        self.nth(n).is_some_and(|t| {
            t.kind == K::Name
                && if self.fteqcc() {
                    self.keywords_ahead(n).checks(enabled, word, &t.text)
                } else {
                    t.text == word
                }
        })
    }

    /// [`Self::checks`] of a keyword's own flag.
    fn kw(&self, n: usize, keyword: Keyword, word: &str) -> bool {
        self.checks(n, Enabled::By(keyword), word)
    }

    /// `QCC_PR_CheckName(word)` of the token `n` ahead (FTEF-20.2): `word`, in any case under
    /// `caseinsens`. Only the FTEQCC family's constructs read it (enums, attributes,
    /// accessors), so the keywords are always FTEQCC's.
    fn names(&self, n: usize, word: &str) -> bool {
        self.nth(n)
            .is_some_and(|t| t.kind == K::Name && self.keywords_ahead(n).names(word, &t.text))
    }

    /// `keyword_X && !STRCMP(word, pr_token)` of the token `n` ahead (FTEF-20.14): `word` exactly,
    /// with its keyword on; no `__` spelling and no other case.
    fn strcmp_kw(&self, n: usize, keyword: Keyword, word: &str) -> bool {
        self.nth(n).is_some_and(|t| {
            t.kind == K::Name && t.text == word && self.keywords_ahead(n).has(keyword)
        })
    }

    /// `QCC_PR_DummyDef` of a definition named `name` (FTEF-20.4): a name of its `KEYWORD` list
    /// turns that keyword off from the next token when `kce` is off. The definition's warning
    /// is the semantic model's, which knows whether it is a new definition.
    fn define(&mut self, name: &str, function: bool) {
        if !self.fteqcc() {
            return;
        }
        if let Some(keyword) = self.keywords.named_definition(name, function) {
            let after = self.keywords.after_definition(keyword);
            if after != self.keywords {
                self.keywords = after;
                // At the current token, after any pragma's mark there, so the marks stay in
                // order.
                let at = self.current_span().start;
                self.keyword_marks.push((at, after));
            }
        }
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
        // `..T`, a field of a field, is one token under FTEQCC (FTEF-18.18).
        if matches!(tok.kind, K::Dot | K::DotDot) {
            return true;
        }
        if tok.kind != K::Name || self.type_word(n).is_none() {
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

    /// Whether the current token starts a declaration at column 0, where recovery resumes:
    /// a declaration's shape, or under FTEQCC a keyword that starts a file-level construct
    /// (FTEF-18.27).
    fn at_declaration_line(&self) -> bool {
        self.current().is_some_and(|t| t.column0)
            && (self.looks_like_declaration(0) || (self.fteqcc() && self.at_fte_construct()))
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
        self.sync_keywords();
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

    /// Reports a warning. A warning does not mute the errors after it, since it does not
    /// abandon the statement or declaration it is in.
    fn warning_at(&mut self, code: Code, message: impl Into<String>, span: Span) {
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

    /// Whether `name` is "name too long" to `PR_ParseName`, `strlen(pr_token) >= MAX_NAME-1`:
    /// id's `MAX_NAME` is 64 (QC-3.3), FTEQCC's 256 (FTEF-15.7).
    fn too_long(&self, name: &str) -> bool {
        let max = if self.fteqcc() {
            FTEQCC_MAX_NAME
        } else {
            LIMIT_QCC_MAX_NAME
        };
        name.len() >= max.saturating_sub(1)
    }

    /// Whether the parser is in an FTEQCC dialect, so the FTEQCC constructs (FTE-2 to FTE-16)
    /// are parsed rather than rejected.
    fn fteqcc(&self) -> bool {
        self.dialect.has_fteqcc_extensions()
    }

    /// Reports an FTEQCC keyword construct used under the vanilla profile (QCD-6.10, E0510).
    ///
    /// The span is the construct's keyword. The FTEQCC dialect accepts the construct and
    /// never calls this; the vanilla dialect reports it once and then parses the construct
    /// the same way, so the tree keeps its shape (QCD-1.4).
    fn extension(&mut self, name: &str, span: Span) {
        self.error_at(
            Code::E0510,
            format!("{name} is an FTEQCC extension, not in vanilla QuakeC"),
            span,
        );
    }

    /// Enters one level of nesting; false, with the error reported, past the limit
    /// (QC-27.15), and false from then on until the next statement, declaration or member
    /// starts ([`Parser::start_item`], QC-27.16). Each call is paired with a [`Parser::leave`].
    fn enter(&mut self) -> bool {
        self.depth = self.depth.saturating_add(1);
        if self.too_deep {
            return false;
        }
        if self.depth > QC_MAX_NESTING {
            self.error(Code::E0509, "nesting too deep");
            self.too_deep = true;
            false
        } else {
            true
        }
    }

    /// Starts a statement, declaration or member: the errors of the last one no longer mute
    /// this one's (QC-24.2), and its nesting starts afresh (QC-27.16).
    fn start_item(&mut self) {
        self.recovering = false;
        self.muted = false;
        self.too_deep = false;
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
                K::RBrace if depth == 0 && (in_block || self.open_blocks > 0) => break,
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
        while self.current().is_some() {
            self.start_item();
            let before = self.pos;
            self.item();
            if self.pos == before {
                self.bump_error();
            }
        }
        self.flush();
        self.finish_node();
    }

    /// One file-level item, what `QCC_PR_ParseDefs` reads at file level: a declaration or one of
    /// the FTEQCC dialect's file-level constructs.
    fn item(&mut self) {
        if self.fteqcc() && !self.defs_start() {
            // FTE-17.3, FTEF-21.17: `QCC_PR_ParseDefs` skips any `;` and then reads a definition.
            return;
        }
        if self.fteqcc() && self.at_extern_block() {
            // FTEF-21.17: `extern` and a string, as the first qualifier.
            self.extern_block(Site::File);
        } else if self.fteqcc() && self.at_typedef() {
            self.typedef_decl(Site::File);
        } else if self.fteqcc() && self.at_enum_body() {
            self.enum_decl();
        } else if self.fteqcc() && self.at_class() {
            // FTEF-9.1: a class, or its forward declaration, after any qualifiers.
            self.class_decl();
        } else if self.fteqcc() && self.at_accessor() {
            // FTEF-10.1: an accessor type.
            self.accessor_decl();
        } else if self.fteqcc()
            && self.at_struct_keyword(0)
            && (self.nth(1).is_some_and(|t| t.kind == K::LBrace)
                || (self.nth(1).is_some_and(|t| t.kind == K::Name)
                    && self
                        .nth(2)
                        .is_some_and(|t| matches!(t.kind, K::LBrace | K::Semicolon | K::Colon))))
        {
            // FTEF-6.1: a named struct or union, or its forward declaration.
            self.struct_decl();
        } else if self.starts_declaration() {
            self.declaration();
        } else {
            let message = format!("\"{}\" is not a type", self.found());
            self.error(Code::E0203, message);
            self.skip(false, true);
        }
    }

    /// Whether a typedef starts here: `typedef`, with its keyword, among the qualifiers
    /// (FTEF-21.14).
    fn at_typedef(&self) -> bool {
        self.at_typedef_at(0)
    }

    /// [`Self::at_typedef`] at the token `n` ahead.
    fn at_typedef_at(&self, n: usize) -> bool {
        let mut n = n;
        while self.decl_qualifier(n) {
            if self.kw(n, Keyword::Typedef, "typedef") {
                return true;
            }
            n = n.saturating_add(1);
        }
        false
    }

    /// Whether an `extern` block starts here (FTEF-21.17): `extern`, with its keyword, then a
    /// string, `if (!hadmodifier && pr_token_type == tt_immediate &&
    /// pr_immediate_type==type_string)`, so only as the first qualifier.
    fn at_extern_block(&self) -> bool {
        self.at_extern_block_at(0)
    }

    /// [`Self::at_extern_block`] at the token `n` ahead.
    fn at_extern_block_at(&self, n: usize) -> bool {
        self.kw(n, Keyword::Extern, "extern")
            && self
                .nth(n.saturating_add(1))
                .is_some_and(|t| t.kind == K::StringLit)
    }

    /// The start of one `QCC_PR_ParseDefs`: its `while (QCC_PR_CheckToken(";"))`, then a
    /// definition, which the end of the text is not, the error `QCC_PR_ParseType` gives
    /// (FTEF-21.17, `"" is not a type`). False, with the error reported, when none follows. A `}`
    /// is no definition either, but its error, `"}" is not a type`, is the one the definition's
    /// own parse gives.
    fn defs_start(&mut self) -> bool {
        while self.at(K::Semicolon) {
            self.bump();
        }
        if self.current().is_some() {
            return true;
        }
        let message = format!("\"{}\" is not a type", self.found());
        self.error(Code::E0203, message);
        false
    }

    /// `extern "C" { ... }`, `extern "C"` and one definition, and the same for `"QC"` and any
    /// other string (FTEF-21.17), at `site`: the string's parts joined, as `QCC_PR_LexString`
    /// joins them, and compared in any case up to the first NUL. The flags it sets hold from the
    /// token after the string to the token after the one that follows the block, as FTEQCC
    /// lexed them; the block is recorded for the pass that lexes the text with them. Each block
    /// is one level of nesting (FTEF-18.32).
    fn extern_block(&mut self, site: Site) {
        if self.enter() {
            self.extern_block_inner(site);
        } else {
            self.bump_error();
        }
        self.leave(1);
    }

    fn extern_block_inner(&mut self, site: Site) {
        self.start_node(K::ExternBlock);
        if !self.at_extern_block() {
            // `local extern "C" ...`: `QCC_PR_ParseStatement` reads `local` and then
            // `QCC_PR_ParseDefs`, whose first qualifier is the `extern`.
            self.bump();
        }
        self.bump();
        let mut text = Vec::new();
        let mut first = true;
        while let Some(tok) = self.current().filter(|t| {
            t.kind == K::StringLit && (first || self.seams.binary_search(&t.span.start).is_err())
        }) {
            first = false;
            text.extend(tok.text.chars().map(latin1_byte));
            self.bump();
        }
        let linkage = Linkage::of(&text);
        let start = self.current_span().start;
        if self.at(K::LBrace) {
            self.bump();
            self.open_blocks = self.open_blocks.saturating_add(1);
            loop {
                if self.at(K::RBrace) {
                    self.bump();
                    break;
                }
                if self.current().is_none() {
                    // `if (pr_token_type == tt_eof) { QCC_PR_Expect ("}"); break; }`
                    self.expected("}");
                    break;
                }
                self.start_item();
                let before = self.pos;
                self.block_item(site);
                if self.pos == before {
                    self.bump_error();
                }
            }
            self.open_blocks = self.open_blocks.saturating_sub(1);
        } else {
            // `QCC_PR_ParseDefs(NULL, false);`
            self.block_item(site);
        }
        // The token after the block's last is lexed before the flags are restored, and read
        // after.
        let lookahead = self.current_span().start;
        let end = self
            .nth(1)
            .map_or(self.input.source.len(), |t| t.span.start);
        self.blocks.push(Block {
            start,
            lookahead,
            end,
            linkage,
        });
        self.finish_node();
    }

    /// One definition of an `extern` block at `site`: an item at file level, a local definition
    /// in a body (FTEF-21.17).
    fn block_item(&mut self, site: Site) {
        if !self.defs_start() {
            return;
        }
        match site {
            Site::File => self.item(),
            Site::Body | Site::Member => {
                if self.at_extern_block() {
                    self.extern_block(site);
                } else if self.fteqcc() && self.at_typedef() {
                    // `QCC_PR_ParseDefs` reads `typedef` among the qualifiers (FTEF-21.14).
                    self.typedef_decl(site);
                } else {
                    self.local_decl(false);
                }
            }
        }
    }

    /// Whether the current token starts a top-level declaration. Vanilla: a `.` or a type
    /// keyword (QC-11.1). FTEQCC also starts one at a `const`/`var`/`nosave` qualifier
    /// (FTE-10 to FTE-12) or at a user-defined type name followed by a name (FTE-14.2), such
    /// as `poseInfo poses[]`.
    fn starts_declaration(&self) -> bool {
        let Some(tok) = self.current() else {
            return false;
        };
        if matches!(tok.kind, K::Dot | K::DotDot) {
            return true;
        }
        if tok.kind == K::LBracket && self.fteqcc() {
            return self.fte_starts_declaration();
        }
        if tok.kind != K::Name {
            return false;
        }
        if self.type_word(0).is_some() {
            return true;
        }
        if !self.fteqcc() {
            return false;
        }
        // FTEF-15.1: at file level every declaration starts with its qualifiers and a type,
        // and a name that is no statement word may be a type the program defines.
        self.fte_starts_declaration()
    }

    /// `[const|var|nosave] type name [array] [= init] {, ...} ;` (QC-11.1, QC-11.2; FTE-10 to
    /// FTE-13).
    fn declaration(&mut self) {
        self.start_node(K::Declaration);
        // FTEF-19.21: after a qualifier the type may be left out, an `int`.
        if self.fteqcc() && self.fte_qualifiers() && self.fte_type_omitted() {
            self.declarators(TypeShape::default(), Site::File);
            self.finish_node();
            return;
        }
        match self.type_or_user_type() {
            Some(ty) => self.declarators(ty, Site::File),
            None => self.recover(false),
        }
        self.finish_node();
    }

    /// A vanilla type (QC-10) or, in an FTEQCC dialect, the full dialect's types, a
    /// user-defined type name among them (FTE-14.2; FTEF-2, FTEF-4, FTEF-7).
    fn type_or_user_type(&mut self) -> Option<TypeShape> {
        // FTEF-20.4: a declaration's body defines only the parameters of its own type.
        self.params.clear();
        if self.fteqcc() {
            self.fte_decl_type()
        } else {
            self.type_()
        }
    }

    /// `typedef struct { T1 f1; ... } Name;` (FTE-14.1), and a `typedef` of an enum or of any
    /// type (FTEF-7.1), with the declarators of any definition (FTEF-7.3): `QCC_PR_ParseDefs`
    /// reads `typedef` among its qualifiers and loops over its declarators, each of which
    /// defines a type. Only the FTEQCC dialect reads it.
    fn typedef_decl(&mut self, site: Site) {
        self.start_node(K::TypedefDecl);
        if self.kw(0, Keyword::Local, "local") {
            // `local typedef ...`: `QCC_PR_ParseStatement` reads `local`, then `QCC_PR_ParseDefs`.
            self.bump_as(K::LocalKw);
        }
        // FTEF-21.14: `QCC_PR_ParseDefs` reads `typedef` in its loop of qualifiers, before or
        // after the others, and any number of times.
        self.fte_qualifiers();
        // FTEF-19.9: `typedef;` defines no type, which `QCC_PR_ParseDefs` warns of.
        if self.at(K::Semicolon) {
            self.bump();
            self.finish_node();
            return;
        }
        let tagged_body = self.nth(1).is_some_and(|t| t.kind == K::LBrace)
            || (self.nth(1).is_some_and(|t| t.kind == K::Name)
                && self.nth(2).is_some_and(|t| t.kind == K::LBrace));
        let mut shape = TypeShape::default();
        // FTEF-19.21: `typedef` is a modifier, so after it the type may be left out, an `int`.
        let omitted =
            self.fte_type_omitted() && self.type_word(0).is_none() && !self.c_type_word(0);
        match (omitted, self.at_struct_keyword(0) && tagged_body) {
            (true, _) => {}
            (false, true) => self.struct_type(),
            (false, false) if self.at_enum_keyword(0) => {
                // FTEF-7.1: `typedef enum { ... } Name;`.
                self.enum_body();
            }
            (false, false) => {
                // FTEF-7.1: `typedef <type> Name;`, any type.
                shape = self.type_or_user_type().unwrap_or_default();
            }
        }
        self.in_typedef = true;
        self.declarators(shape, site);
        self.in_typedef = false;
        self.finish_node();
    }

    /// `QCC_PR_ParseStruct` (FTE-14.1, FTEF-6): `struct` or `union`, then `{` for an anonymous
    /// one, or a tag, a parent after `:` for a struct (which needs the body), and the body, or
    /// nothing after a tag, which names the type (FTEF-6.1, FTEF-6.8). The members are read as
    /// `QCC_PR_ParseStruct`'s loop reads them (FTEF-6.7).
    fn struct_type(&mut self) {
        self.start_node(K::StructType);
        let union = self.kw(0, Keyword::Union, "union");
        self.bump_as(if union { K::UnionKw } else { K::StructKw });
        if self.at(K::LBrace) {
            self.bump();
            self.struct_members(if union { "union" } else { "struct" }, "");
            self.finish_node();
            return;
        }
        // `char *tname = QCC_PR_ParseName();`
        let tag = self.found();
        if !self.name() {
            self.finish_node();
            return;
        }
        self.declare_type(tag.clone());
        let mut parent = false;
        if !union && self.at(K::Colon) {
            // FTEF-6.3: `struct Child : Parent`; a union takes no parent.
            self.bump();
            parent = true;
            if !self.name() {
                self.finish_node();
                return;
            }
        }
        // `if (parenttype) QCC_PR_Expect("{"); else if (!QCC_PR_CheckToken("{")) return newt;`
        let opened = if parent {
            self.expect(K::LBrace, "{")
        } else if self.at(K::LBrace) {
            self.bump();
            true
        } else {
            false
        };
        if opened {
            self.struct_members(if union { "union" } else { "struct" }, &tag);
        }
        self.finish_node();
    }

    /// The members of a struct's body, up to and including its `}` (FTEF-6.7).
    fn struct_members(&mut self, kind: &str, tag: &str) {
        // `newparm`: whether a member's type is open for a `,`, until its `;`.
        let mut open = false;
        // `numparms`: the members counted, which a `static` one is not (FTEF-6.7).
        let mut members = 0_usize;
        let mut statics = false;
        loop {
            if self.current().is_none() {
                self.expected("}");
                return;
            }
            if self.at(K::RBrace) {
                if open {
                    self.expected(";");
                }
                if members == 0 {
                    let name = if tag.is_empty() {
                        format!("<{kind}>")
                    } else {
                        tag.to_owned()
                    };
                    self.error(Code::E0243, format!("{kind} {name} has no members"));
                }
                self.bump();
                return;
            }
            if self.lost_brace() {
                // FTEF-18.27: a struct that lost its `}` ends at the next declaration line.
                self.expect(K::RBrace, "}");
                return;
            }
            if self.at(K::Semicolon) {
                self.bump();
                open = false;
                continue;
            }
            let before = self.pos;
            if self.at(K::Comma) {
                if !open {
                    self.error(Code::E0201, "element missing type");
                    self.bump_error();
                    return;
                }
                if self.after_bitfield {
                    // A bitfield's type is `bitfld` and no `,` goes on with it (FTEF-6.9).
                    self.error(Code::E0204, "expected ;");
                }
                self.start_node(K::StructField);
                self.bump();
                self.struct_declarator();
                self.finish_node();
                members = members.saturating_add(usize::from(!statics));
            } else {
                if open {
                    // "missing semi-colon" (FTEF-6.7): the member is read on (QCD-1.4).
                    self.expected(";");
                    open = false;
                }
                if self.member_label() {
                    continue;
                }
                self.after_bitfield = false;
                self.start_node(K::StructField);
                (open, statics) = self.struct_member();
                self.finish_node();
                members = members.saturating_add(usize::from(open && !statics));
            }
            if self.pos == before {
                self.bump_error();
                return;
            }
        }
    }

    /// `public:`, `private:` and `protected:` in a struct, `QCC_PR_CheckKeyword(1, ...)` and
    /// `QCC_PR_Expect(":")` (FTEF-6.7). Whether it read one.
    fn member_label(&mut self) -> bool {
        if !["public", "private", "protected"]
            .iter()
            .any(|word| self.checks(0, Enabled::Always, word))
        {
            return false;
        }
        self.start_node(K::AccessLabel);
        self.bump();
        self.expect(K::Colon, ":");
        self.finish_node();
        true
    }

    /// One new type of a struct's body and its first declarator: `static` or `virtual`, then
    /// `QCC_PR_ParseType(false, false, true)`, the `*`s, and a name, `(name)` or nothing before
    /// the `;` (FTEF-6.7). Whether a `,` may continue it.
    fn struct_member(&mut self) -> (bool, bool) {
        let mut statics = false;
        self.member_global = false;
        while ["static", "virtual"]
            .iter()
            .any(|word| self.checks(0, Enabled::Always, word))
        {
            statics |= self.checks(0, Enabled::Always, "static");
            self.member_global = true;
            self.start_node(K::Qualifier);
            self.bump();
            self.finish_node();
        }
        let Some(ty) = self.fte_member_type() else {
            self.recover(true);
            return (false, statics);
        };
        self.member_function = ty.function;
        if ty.qcfuncs && self.at(K::LParen) && self.nth(1).is_some_and(|t| t.kind == K::Star) {
            // `ParseType` with `qcfuncs` took the `(`, and `ParseFunctionType` found a `*`.
            self.error(Code::E0204, "expected function arg list");
            self.recover(true);
            return (false, statics);
        }
        self.struct_declarator();
        (true, statics)
    }

    /// What follows a struct member's type, or a `,`: `*`s, then a name, `(*name)` or nothing,
    /// array suffixes, a parameter list and a bitfield width (FTEF-6.4, FTEF-6.7).
    fn struct_declarator(&mut self) {
        while self.at(K::Star) {
            self.bump();
        }
        if self.at(K::Semicolon) {
            // An anonymous member: a struct or union, or a member with no name.
            return;
        }
        if self.at(K::LParen) {
            self.bump();
            if self.at(K::Star) {
                self.bump();
            }
            self.name();
            self.expect(K::RParen, ")");
        } else {
            self.name();
        }
        while self.at(K::LBracket) {
            if self.nth(1).is_some_and(|t| t.kind == K::RBracket) {
                // `QCC_PR_IntConstExpr()` reads the size, which `]` is none of.
                self.error(Code::E0201, "not a name");
            }
            self.array_suffix();
        }
        let mut function = self.member_function;
        if self.at(K::LParen) {
            // FTEF-6.4: a member of function type, `float Get(int);`.
            if self.at_function_list() {
                self.fte_param_list();
                function = true;
            } else {
                self.error(Code::E0204, "expected function arg list");
            }
        }
        if self.at(K::Colon) {
            // FTEF-6.4: a bitfield `int name : bits;`.
            self.bump();
            self.expression();
            self.after_bitfield = true;
        }
        if self.member_global {
            // A `static` or `virtual` member is a global of the struct's, `Struct::member`:
            // `if (QCC_PR_CheckToken("=") || (type->type == ev_function &&
            // QCC_PR_PeekToken("{"))) QCC_PR_ParseInitializerDef(d, 0);`, a body or a value.
            if self.at(K::Eq) {
                self.bump();
                if function && (self.at(K::LBrace) || self.at(K::LBracket)) {
                    self.function_body();
                } else {
                    self.expression();
                }
            } else if function && self.at(K::LBrace) {
                self.function_body();
            }
        } else if self.at(K::Eq) {
            // `defaultval = QCC_PR_ParseDefaultInitialiser (type); QCC_PR_ParseWarning
            // (ERR_INTERNAL, "TODO: pre-initialised struct members are not implemented yet");`,
            // an error.
            self.bump();
            self.expression();
            self.error(
                Code::E0204,
                "TODO: pre-initialised struct members are not implemented yet",
            );
        }
    }

    /// Whether the body being read lost its `}` (FTEF-18.27): a declaration's shape at column 0
    /// and no `}` ahead to close the body. Sets [`Self::recovering`] when it did.
    fn lost_brace(&mut self) -> bool {
        let lost = self.at_declaration_line() && !self.closes_ahead();
        self.recovering |= lost;
        lost
    }

    /// Whether a block's statements end at a declaration line: under the vanilla profile
    /// always, where a declaration needs `local`; under FTEQCC's only when the block lost its
    /// `}` (FTEF-18.27), since a declaration there is a statement.
    fn ends_at_declaration_line(&mut self) -> bool {
        if self.fteqcc() {
            self.lost_brace()
        } else {
            self.at_declaration_line()
        }
    }

    /// Whether a `}` that matches the current open brace stands ahead: the `}` the innermost
    /// `{` before this token pairs with, found once for every token (`brace_close`).
    fn closes_ahead(&self) -> bool {
        self.brace_close
            .get(self.pos)
            .is_some_and(|close| close.is_some())
    }

    /// `enum [class] [Name] [: type] { A, B = n, ... };` (FTE-15.1, FTE-15.2, FTE-15.4). The
    /// `class` and the `: type` are accepted and otherwise ignored for a version 6 target;
    /// the name is a type name (FTE-15.4).
    fn enum_decl(&mut self) {
        self.start_node(K::EnumDecl);
        self.enum_head_and_body();
        // FTEF-8.5: `enum E { A } x;` declares a variable of the enum, as any type is followed
        // by declarators (FTEF-15.6).
        let shape = self.type_decl_tail();
        self.declarators(shape, Site::File);
        self.finish_node();
    }

    /// `enum [class] [Name] [: type] { ... }` as the type of a `typedef` (FTEF-7.1), in an
    /// `EnumDecl` node.
    fn enum_body(&mut self) {
        self.start_node(K::EnumDecl);
        self.enum_head_and_body();
        self.finish_node();
    }

    /// The keyword, name, base type and enumerators of an `enum` or `enumflags`.
    fn enum_head_and_body(&mut self) {
        self.bump_as(K::EnumKw);
        // `strictenum = QCC_PR_CheckName("class");`
        let strict = self.names(0, "class");
        if strict {
            self.bump();
        }
        if self.at(K::Name) {
            self.declare_type(self.found());
            self.bump();
        }
        if self.at(K::Colon) {
            // `: int` or another underlying type, accepted and ignored (FTE-15.2).
            self.bump();
            self.fte_type();
        } else if strict && !self.expect(K::Colon, ":") {
            // `else if (strictenum) QCC_PR_Expect(":");` (FTEF-8.5)
            return;
        }
        if self.expect(K::LBrace, "{") {
            // `while(1) { name = QCC_PR_ParseName(); ...; if (QCC_PR_CheckToken("}")) break;
            // QCC_PR_Expect(","); if (QCC_PR_CheckToken("}")) break; }`: at least one name, and
            // a `,` may end the list (FTEF-19.37).
            loop {
                self.start_node(K::Enumerator);
                // FTEF-20.4: `QCC_PR_ParseEnum` makes each name with `QCC_PR_DummyDef`.
                let name = self.found();
                let named = self.name();
                if named {
                    self.define(&name, false);
                }
                if named && self.at(K::Eq) {
                    self.bump();
                    self.expression();
                }
                self.finish_node();
                if !named {
                    return;
                }
                if self.at(K::RBrace) {
                    self.bump();
                    return;
                }
                if !self.expect(K::Comma, ",") {
                    return;
                }
                if self.at(K::RBrace) {
                    self.bump();
                    return;
                }
            }
        }
    }

    fn declarators(&mut self, ty: TypeShape, site: Site) {
        if self.recovering {
            return;
        }
        // FTEF-20.12: `QCC_PR_ParseDefs` takes a `;` right after the type and defines nothing,
        // so a type may stand alone, an enum's or a struct's in a body among them; a class's
        // member is `QCC_PR_ParseEntClass`'s, which needs a name (FTEF-9.4).
        if self.fteqcc() && site != Site::Member && self.at(K::Semicolon) {
            self.bump();
            return;
        }
        let mut first = true;
        let function = loop {
            let function = self.declarator(ty, site, first);
            first = false;
            if !self.at(K::Comma) {
                break function;
            }
            self.bump();
            // FTEF-15.8: FTEQCC's declarator loop starts with `if (QCC_PR_CheckToken (";"))`
            // and returns, so a `,` may end the list; `QCC_PR_ParseEntClass`'s loop reads a
            // name after the `,` instead (`parmname = QCC_PR_ParseName();`, FTEF-9.9).
            if self.fteqcc() && self.at(K::Semicolon) {
                if site == Site::Member {
                    self.error(Code::E0201, "\";\" - not a name");
                }
                self.bump();
                return;
            }
        };
        if self.at(K::Semicolon) {
            self.bump();
            return;
        }
        if self.fteqcc() && !self.muted && site != Site::Member {
            // FTE-17.3: after a function declaration `QCC_PR_ParseDefs` only checks for a
            // `;`. FTE-21.3: after any other definition a missing `;` is FTEQCC's warning
            // "Missing semicolon at end of definition", and the next statement or
            // definition follows.
            if !function {
                let span = Span::new(self.last_end, self.last_end);
                self.warning_at(Code::W0203, "Missing semicolon at end of definition", span);
            }
            return;
        }
        self.expected(";");
        self.recover(site.in_block());
    }

    /// Parses one declarator; returns whether its type is a function type, whose `;` an
    /// FTEQCC declaration may leave out (FTE-17.3). A definition's name is
    /// `QCC_PR_DummyDef`'s, once its type is known (FTEF-20.4).
    fn declarator(&mut self, ty: TypeShape, site: Site, first: bool) -> bool {
        self.start_node(K::Declarator);
        let mut ty = ty;
        let pointer = self.declarator_head(&mut ty, first);
        let mut name = self.found();
        let named = self.name();
        if pointer {
            // `name = QCC_PR_ParseName (); if (QCC_PR_CheckToken ("[")) { ... } QCC_PR_Expect
            // (")"); ... if (QCC_PR_CheckToken ("(")) type = QCC_PR_ParseFunctionType(...)`
            if self.at(K::LBracket) {
                self.array_suffix();
            }
            self.expect(K::RParen, ")");
            if self.at_function_list() {
                self.fte_param_list();
                ty.function = true;
            }
        }
        // FTEF-9.4: `Class::member` names a member a class declares, whose definition is named
        // `Class::member` (FTEF-20.4).
        while self.fteqcc() && !self.in_typedef && self.at(K::ColonColon) {
            self.bump();
            name = format!("{name}::{}", self.found());
            self.name();
        }
        // FTEQCC array suffix `[N]` or `[]` after the declared name (FTE-13.1), for every type:
        // `if (!arraysize && QCC_PR_CheckToken ("["))` comes before a function's `[` state
        // header is looked for (FTE-17.7). Vanilla uses `[` only in a state header
        // `=[frame, think]`, after `=`, so this does not change vanilla.
        let mut array = false;
        while self.fteqcc() && self.at(K::LBracket) {
            self.array_suffix();
            array = true;
        }
        // FTE-17.1: the C-style function declarator `rettype name(params)`, which the
        // rerelease uses beside the vanilla `rettype(params) name`, and FTEF-5.3: `T
        // name[N](params)`, an array of functions. A `(` after the name or its array
        // suffixes, on a non-function type, is the parameter list, and the declarator is then
        // a function.
        if self.fteqcc() && !ty.function && self.at(K::LParen) {
            self.fte_param_list();
            ty.function = true;
        }
        if self.in_typedef {
            // FTEF-7.3: a `typedef`'s declarator defines the type `name`, a function type when
            // the type or the declarator's parameter lists make one (FTEF-7.5); an array of
            // functions is no function.
            if named {
                self.declare_type_shaped(name, ty.function && !array);
            }
            // `if (istypedef) { ...; def = NULL; continue; }`: no initialiser or body is read.
            self.finish_node();
            return ty.function;
        }
        // FTE-17.7: an array of functions is initialised as an array, by an aggregate, not by
        // a body.
        if array {
            ty.function = false;
        }
        if site != Site::Member {
            self.define(&name, ty.function);
        }
        if self.at(K::Eq) {
            self.bump();
            self.initializer(ty);
        } else if ty.function && (self.at(K::LBrace) || self.at(K::LBracket)) {
            // FTE-17.2: FTEQCC takes a `{` or `[` after a function declarator as its body,
            // with no `=`. Vanilla `PR_ParseDefs` finds no `=` or `,` and expects `;`; the
            // body is kept.
            if !self.fteqcc() {
                self.expected(";");
            }
            self.function_body();
        } else if ty.function && self.fteqcc() && self.at(K::Colon) {
            // FTE-17.2, FTE-17.5: a `:` after a function declarator is its builtin
            // initialiser, with no `=`.
            self.builtin();
        }
        self.finish_node();
        ty.function
    }

    /// What `QCC_PR_ParseDefs` reads of a declarator before its name (FTEF-4.1, FTEF-21.7):
    /// its `*`s, each a pointer (`T a, *b` declares `b` one, FTEF-18.32), then each parameter list
    /// of its `while(QCC_PR_CheckToken ("("))`, a function returning the type so far, which marks
    /// `ty` a function's, up to `(*`, C's function pointer, whose `(` and `*`s it takes. Returns
    /// whether it took them, so the name, a `)` and a parameter list follow. When the type is
    /// the first declarator's of a type `QCC_PR_ParseType` read with `qcfuncs` on, the type took
    /// any parameter list, and its `ParseFunctionType` finds no list at `(*`, which this leaves
    /// for the name to refuse.
    fn declarator_head(&mut self, ty: &mut TypeShape, first: bool) -> bool {
        let mut levels = 0_usize;
        while self.fteqcc() && self.at(K::Star) {
            levels = levels.saturating_add(1);
            if !self.enter() {
                break;
            }
            self.bump();
        }
        let mut pointer = false;
        while self.fteqcc() && self.at(K::LParen) {
            levels = levels.saturating_add(1);
            if !self.enter() {
                break;
            }
            if self.nth(1).is_some_and(|t| t.kind == K::Star) {
                if first && ty.qcfuncs && !ty.function {
                    break;
                }
                // `else { //c-style function pointer while (QCC_PR_CheckToken("*")) isptr++;`
                self.bump();
                while self.at(K::Star) {
                    self.bump();
                }
                pointer = true;
                break;
            }
            self.fte_param_list();
            ty.function = true;
        }
        self.leave(levels);
        pointer
    }

    /// Whether a parameter list starts here: a `(` that opens no `(*name)` (FTEF-15.3).
    fn at_function_list(&self) -> bool {
        self.at(K::LParen) && !self.nth(1).is_some_and(|t| t.kind == K::Star)
    }

    /// `[N]` or `[]` after a declared name (FTE-13.1): a fixed size or a size inferred from
    /// the initialiser.
    fn array_suffix(&mut self) {
        self.start_node(K::ArraySuffix);
        self.bump_as(K::LBracket);
        if !self.at(K::RBracket) {
            self.expression();
        }
        self.expect(K::RBracket, "]");
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
        if self.too_long(&tok.text) {
            self.error(Code::E0202, "name too long");
        }
        self.bump();
        true
    }

    fn initializer(&mut self, ty: TypeShape) {
        if ty.function {
            // FTE-17.5: FTEQCC opens a builtin initialiser with `#` or `:`.
            if self.at(K::Hash) || (self.fteqcc() && self.at(K::Colon)) {
                self.builtin();
            } else if self.fteqcc() && !self.at(K::LBrace) && !self.at(K::LBracket) {
                // FTEF-15.2: an FTEQCC function may be initialised with another function or
                // `0`, as `QCC_PR_ParseInitializerType` reads a value.
                self.expression();
            } else {
                self.function_body();
            }
            return;
        }
        // FTEQCC aggregate initialiser `{ a, b, ... }` for an array or struct (FTE-13.1,
        // FTE-14.3). Vanilla has no such form, so a `{` after `=` on a non-function stays the
        // "initialiser is not a constant" case below.
        if self.fteqcc() && self.at(K::LBrace) {
            self.aggregate_init();
            return;
        }
        let start = self.current_span().start;
        let state = self.at(K::LBracket) && !self.fteqcc();
        if self.at(K::LBrace) || state {
            self.function_body();
        } else if self.at(K::Hash) {
            self.builtin();
        } else {
            match self.expression() {
                Some(expr) if expr.kind == K::Literal => return,
                // FTEQCC folds any constant expression as an initialiser (FTE-10.2), such as
                // `const float B = A;` or `LightStyles.length - 1`, so a non-literal
                // expression is accepted in an FTEQCC dialect.
                Some(_) if self.fteqcc() => return,
                Some(_) => {}
                None => return,
            }
        }
        let span = Span::new(start, self.last_end.max(start));
        self.error_at(Code::E0506, "initialiser is not a constant", span);
    }

    /// `{ a, b, ... }`: an FTEQCC array or struct initialiser (FTE-13.1, FTE-14.3). Each
    /// element is an expression or a nested aggregate `{ ... }` (an array of structs). A
    /// trailing comma before `}` is allowed. Only the FTEQCC dialect reads it.
    fn aggregate_init(&mut self) {
        if !self.enter() {
            self.bump_error();
            self.leave(1);
            return;
        }
        self.start_node(K::AggregateInit);
        self.bump_as(K::LBrace);
        while !self.at(K::RBrace) && self.current().is_some() {
            if self.at(K::Dot)
                && self.nth(1).is_some_and(|t| t.kind == K::Name)
                && self.nth(2).is_some_and(|t| t.kind == K::Eq)
            {
                // FTEF-16.11: `.member = value` names the member it initialises.
                self.bump();
                self.bump();
                self.bump();
            }
            if self.at(K::LBrace) {
                self.aggregate_init();
            } else {
                let before = self.pos;
                self.expression();
                if self.pos == before {
                    self.bump_error();
                    break;
                }
            }
            if !self.at(K::Comma) {
                break;
            }
            self.bump();
        }
        self.expect(K::RBrace, "}");
        self.finish_node();
        self.leave(1);
    }

    /// `#number` (QC-13.3, QC-27.9), and in an FTEQCC dialect `#number:name`, `#"name"`,
    /// `:name` and the other forms of FTE-17.4, FTE-17.5 and FTE-23.
    fn builtin(&mut self) {
        self.start_node(K::Builtin);
        let colon = self.at(K::Colon);
        let opener = self.current().map(|t| t.span.end);
        self.bump();
        // FTE-23.1: FTEQCC's lexer reads a `#` as a token only right before `"`, `-`, a digit,
        // `)` or `,`; before anything else it is a macro use, an error with no such macro, as
        // the preprocessor has already read a macro's (FTE-23.10). The parser reports only the
        // first error of a declaration, so this one is reported.
        if self.fteqcc() && !colon && !self.hash_token_follows(opener) {
            self.error(Code::E0209, "Bad builtin immediate");
        }
        let number = self.current().map(|t| (t.kind, t.value));
        let binum = self
            .current()
            .and_then(|t| crate::builtin_binum(&t.text, self.dialect));
        let mut named = false;
        match number {
            // FTE-17.5: FTEQCC takes the engine name from a string after `#` or `:`, or from a
            // name after `:`; after `#` its lexer reads a name as a macro use. FTE-23.5: its
            // lexer joins adjacent strings into one, and one empty up to its first NUL names
            // nothing.
            Some((K::StringLit, _)) if self.fteqcc() => {
                let mut first = None;
                while self.at(K::StringLit) {
                    first = first.or_else(|| self.current().and_then(|t| t.text.chars().next()));
                    self.bump();
                }
                named = first.is_some_and(|c| c != '\0');
            }
            Some((K::Name, _)) if colon => {
                named = true;
                self.bump();
            }
            // QC-13.3, QC-13.10, FTE-17.6, FTE-23.3: a literal no compiler binds is "Bad
            // builtin immediate".
            Some((K::FloatLit, Some(value))) => {
                if binum.is_none() {
                    self.error(Code::E0209, "Bad builtin immediate");
                } else if value <= 0.0 && !self.fteqcc() {
                    // FTE-17.4: FTEQCC takes any integral number, 0 included.
                    self.error(Code::E0507, "builtin number must be positive");
                }
                self.bump();
            }
            Some((K::FrameRef, _)) => self.bump(),
            _ => self.error(Code::E0209, "Bad builtin immediate"),
        }
        // FTE-17.4: FTEQCC's `#number:name` binds a builtin by its engine name, as the
        // rerelease's `#0:ex_bprint`; FTE-17.5: only when no name was read yet.
        if self.fteqcc() && !named && self.at(K::Colon) {
            self.bump();
            self.name();
        }
        self.finish_node();
    }

    /// Whether FTEQCC's `QCC_PR_Lex` reads a `#` that ends at `opener` as a token (FTE-23.1):
    /// the byte right after it is `"`, `-`, a digit, `)` or `,`. A string with a prefix
    /// (FTEF-13.7) starts with a name's byte, so it is not one.
    fn hash_token_follows(&self, opener: Option<usize>) -> bool {
        opener
            .and_then(|at| self.input.source.get(at))
            .is_some_and(|&b| matches!(b, b'"' | b'-' | b')' | b',') || b.is_ascii_digit())
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
        // FTEF-20.4: `QCC_PR_ParseImmediateStatements` defines the parameters first.
        for (name, function) in std::mem::take(&mut self.params) {
            self.define(&name, function);
        }
        self.start_node(K::FunctionBody);
        if self.at(K::LBracket) {
            self.state();
        }
        self.block(true);
        self.end_function_labels();
        self.finish_node();
    }

    /// The end of a function, a function defined inside another included (FTEF-18.24): each
    /// `goto` read since the last end must name a label read since then, or it is "Goto
    /// statement with no matching label"; then FTEQCC forgets both (`num_gotos = 0;` and
    /// `if (num_labels) num_labels = 0;` in `QCC_PR_ParseImmediateStatements`).
    fn end_function_labels(&mut self) {
        for (name, span) in std::mem::take(&mut self.gotos) {
            if !self.labels.contains(&name) {
                // Not a parse error: it abandons nothing, so it mutes nothing.
                self.errors.push(Diagnostic {
                    code: Code::E0308,
                    message: format!("Goto statement with no matching label \"{name}\""),
                    span,
                });
            }
        }
        self.labels.clear();
    }

    /// `[frame, next]` (QC-13.6), or under FTEQCC its expressions and frame cycles
    /// (FTEF-18.3, FTEF-18.28).
    fn state(&mut self) {
        if self.fteqcc() {
            self.fte_state();
            return;
        }
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
                qcfuncs: true,
            });
        }
        let Some(keyword) = self.type_word(0) else {
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
                qcfuncs: true,
            });
        }
        self.builder.start_node_at(checkpoint, K::FunctionType);
        self.param_list();
        self.finish_node();
        Some(TypeShape {
            function: true,
            field: false,
            qcfuncs: true,
        })
    }

    /// `( )`, `( ... )` or `( type name {, type name} )` (QC-10.3, QC-10.4, QC-27.7); the
    /// FTEQCC dialect's parameters are [`Parser::fte_param_list`]'s (FTE-18, FTEF-15.3).
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
        if function {
            self.bodies = self.bodies.saturating_add(1);
        }
        self.block_inner(function);
        if function {
            self.bodies = self.bodies.saturating_sub(1);
            if self.bodies == 0 {
                self.types.retain(|t| !t.local);
            }
        }
    }

    /// Records that the program defined the type `name` here (FTEF-20.14).
    fn declare_type(&mut self, name: String) {
        self.declare_type_shaped(name, false);
    }

    /// Records that the program defined the type `name` here, a function type when `function`
    /// (FTEF-7.5).
    fn declare_type_shaped(&mut self, name: String, function: bool) {
        self.types.push(DeclaredType {
            name,
            local: self.bodies > 0,
            function,
        });
    }

    /// Whether the token `n` ahead names a declared type of function type, in any case under
    /// `caseinsens` (FTEF-7.5, FTEF-19.33): the last declaration of the name counts.
    fn declared_function(&self, n: usize) -> bool {
        let any_case = self.any_case(n);
        self.nth(n).is_some_and(|tok| {
            self.types
                .iter()
                .rev()
                .find(|t| {
                    if any_case {
                        t.name.eq_ignore_ascii_case(&tok.text)
                    } else {
                        t.name == tok.text
                    }
                })
                .is_some_and(|t| t.function)
        })
    }

    fn block_inner(&mut self, function: bool) {
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
                // FTE-21.4: FTEQCC's `QCC_PR_ParseStatement` reads statements until the
                // `}`, so its blocks may be empty.
                if !function && count == 0 && !self.fteqcc() {
                    self.error(Code::E0201, "not a name");
                }
                self.bump();
                break;
            }
            if self.ends_at_declaration_line() {
                self.expected("}");
                break;
            }
            self.start_item();
            if self.fteqcc() && self.at_case_label() {
                // FTEF-16.12: a label of the enclosing `switch` may stand in a nested block.
                self.case_label();
                continue;
            }
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
            // FTE-21.5: an FTEQCC `;` alone is an empty statement, with FTEQCC's warning
            // "Hanging ';'".
            K::Semicolon if self.fteqcc() => {
                let span = tok.span;
                self.warning_at(Code::W0204, "Hanging ';'", span);
                self.start_node(K::EmptyStmt);
                self.bump();
                self.finish_node();
            }
            K::Dot => self.local_decl(false),
            // Only the FTEQCC lexer makes the `..` and `:` tokens.
            K::DotDot => self.local_decl(false),
            // FTEF-18.23: `:name`, the FrikQCC label.
            K::Colon => self.label_stmt(),
            K::Name if self.fteqcc() => self.fte_statement(),
            K::Name => match tok.text.as_str() {
                "return" => self.return_stmt(),
                "while" => self.while_stmt(),
                "do" => self.do_stmt(),
                "local" => self.local_decl(true),
                "if" => self.if_stmt(),
                // FTEQCC statement keywords (FTE-2 to FTE-4). Each is recognised only in the
                // position the construct takes, so a name that happens to spell the keyword
                // elsewhere (the `break` builtin name, QC-3.2) still parses as before: `for`
                // and `switch` need a `(`, and `break` and `continue` a `;`.
                "for" if self.nth(1).is_some_and(|t| t.kind == K::LParen) => self.for_stmt(),
                "switch" if self.nth(1).is_some_and(|t| t.kind == K::LParen) => self.switch_stmt(),
                "break" if self.nth(1).is_some_and(|t| t.kind == K::Semicolon) => {
                    self.break_continue_stmt(K::BreakStmt, K::BreakKw, "break");
                }
                "continue" if self.nth(1).is_some_and(|t| t.kind == K::Semicolon) => {
                    self.break_continue_stmt(K::ContinueStmt, K::ContinueKw, "continue");
                }
                _ if self.looks_like_declaration(0) => self.local_decl(false),
                _ => self.expr_stmt(),
            },
            _ => self.expr_stmt(),
        }
    }

    /// A statement of an FTEQCC dialect that starts with a name, as `QCC_PR_ParseStatement`
    /// tests its keywords, in its order, each with the keywords in force (FTEF-20.5, FTEF-20.14).
    fn fte_statement(&mut self) {
        let break_call =
            self.kw(0, Keyword::Break, "break") && self.nth(1).is_some_and(|t| t.kind == K::LParen);
        if self.checks(0, Enabled::Always, "return") {
            self.return_stmt();
        } else if self.checks(0, Enabled::Never, "exit") {
            // `keyword_exit` is `flag_acc`, reacc's, which this profile does not read
            // (FTEF-19.46): only `__exit` is the keyword.
            self.exit_stmt();
        } else if self.kw(0, Keyword::Loop, "loop") {
            self.loop_stmt();
        } else if self.at_until() || self.checks(0, Enabled::Always, "while") {
            self.while_stmt();
        } else if self.kw(0, Keyword::For, "for") {
            self.for_stmt();
        } else if self.checks(0, Enabled::Always, "do") {
            self.do_stmt();
        } else if self.kw(0, Keyword::Local, "local") && self.at_extern_block_at(1) {
            // FTEF-21.17: `local` and then an `extern` block.
            self.extern_block(Site::Body);
        } else if self.kw(0, Keyword::Local, "local") && self.at_typedef_at(1) {
            self.typedef_decl(Site::Body);
        } else if self.kw(0, Keyword::Local, "local") {
            self.local_decl(true);
        } else if self.strcmp_kw(0, Keyword::Typedef, "typedef")
            || (self.fte_looks_like_local() && self.at_typedef())
        {
            // FTEF-19.9: `typedef` in a function body names a type there; FTEF-21.14: after the
            // qualifiers a statement of definitions starts with.
            self.typedef_decl(Site::Body);
        } else if self.strcmp_kw(0, Keyword::Extern, "extern") && self.at_extern_block() {
            // FTEF-21.17: `QCC_PR_ParseStatement` tests `extern` exactly, then
            // `QCC_PR_ParseDefs` reads the block, whose definitions are locals.
            self.extern_block(Site::Body);
        } else if self.fte_looks_like_local() {
            // FTEF-16.1, FTEF-20.14: a statement of the full dialect that declares locals.
            self.local_decl(false);
        } else if self.kw(0, Keyword::State, "state") {
            self.state_stmt();
        } else if self.checks(0, Enabled::Always, "if") {
            self.if_stmt();
        } else if self.kw(0, Keyword::Switch, "switch") {
            self.switch_stmt();
        } else if self.kw(0, Keyword::Using, "using") {
            self.using_stmt();
        } else if self.kw(0, Keyword::Asm, "asm") {
            self.asm_stmt();
        } else if self.kw(0, Keyword::Goto, "goto") {
            self.goto_stmt();
        } else if self.kw(0, Keyword::Break, "break") && !break_call {
            // `if (!STRCMP ("(", pr_token))`: a `break` before `(` is a call of the function
            // `break`, read again as a name.
            self.break_continue_stmt(K::BreakStmt, K::BreakKw, "break");
        } else if self.kw(0, Keyword::Continue, "continue") {
            self.break_continue_stmt(K::ContinueStmt, K::ContinueKw, "continue");
        } else if self.kw(0, Keyword::Thinktime, "thinktime") {
            self.thinktime_stmt();
        } else if self.at_c_label() {
            // FTEF-18.23: `name:`, the C label, its `:` right after the name.
            self.label_stmt();
        } else if self.looks_like_declaration(0) {
            self.local_decl(false);
        } else {
            self.expr_stmt();
        }
    }

    /// Whether a `case` or `default` label starts here, with its keyword (FTEF-20.5).
    fn at_case_label(&self) -> bool {
        self.kw(0, Keyword::Case, "case") || self.kw(0, Keyword::Default, "default")
    }

    /// `for (init; cond; step) body` (FTE-2). `init` is an expression or a local declaration,
    /// `cond` and `step` are expressions, and each clause may be empty.
    fn for_stmt(&mut self) {
        let span = self.current_span();
        if !self.fteqcc() {
            self.extension("for", span);
        }
        self.start_node(K::ForStmt);
        self.bump_as(K::ForKw);
        self.expect(K::LParen, "(");
        self.for_clause(Clause::Init);
        self.expect(K::Semicolon, ";");
        self.for_clause(Clause::Condition);
        self.expect(K::Semicolon, ";");
        self.for_clause(Clause::Step);
        self.expect(K::RParen, ")");
        self.statement();
        self.finish_node();
    }

    /// One clause of a `for` header (FTE-2.1, FTE-2.2): empty, a declaration (init only), or
    /// an expression. Stops at `;` or `)`. FTEQCC discards the first and last clauses
    /// (FTEF-18.25): it reads each expression of the first, separated by `,`, with
    /// `QCC_PR_DiscardExpression(TOP_PRIORITY, EXPR_DISALLOW_COMMA)` in a `do ... while
    /// (QCC_PR_CheckToken(","))` loop, and the last with `QCC_PR_DiscardExpression(TOP_PRIORITY,
    /// 0)`, one sequence; the condition is a sequence whose value is kept.
    fn for_clause(&mut self, clause: Clause) {
        if self.at(K::Semicolon) || self.at(K::RParen) {
            return;
        }
        let init = clause == Clause::Init;
        self.start_node(K::ForClause);
        // FTEF-16.14: `QCC_PR_ParseType` reads no `local`, so under FTEQCC it starts no
        // declaration here.
        let local = !self.fteqcc() && self.kw(0, Keyword::Local, "local");
        let declares =
            local || self.looks_like_declaration(0) || (self.fteqcc() && self.for_init_declares());
        if init && declares {
            let keyword = local;
            self.for_local_decl(keyword);
        } else {
            let outer = std::mem::replace(&mut self.useful, false);
            if clause == Clause::Condition {
                self.useful = outer;
            }
            let mut last = self.for_expression(clause);
            // FTEF-16.9: FTEQCC's `for` clauses may hold several expressions, `i = 3, j = 0`,
            // and the first a declaration after them, `v = 0, int i = 1`.
            while self.fteqcc() && self.at(K::Comma) {
                if let Some(expr) = last.as_ref().filter(|_| !init) {
                    self.discard_before_comma(expr);
                }
                self.bump();
                if init && self.for_init_declares() {
                    self.for_local_decl(false);
                    break;
                }
                last = self.for_expression(clause);
            }
            if clause == Clause::Step
                && let Some(expr) = last
            {
                self.discard_at_end(&expr);
            }
            if clause != Clause::Condition {
                self.useful = outer;
            }
        }
        self.finish_node();
    }

    /// Whether a `for` initialiser's item defines a local: a statement of definitions
    /// (FTEF-16.1), or what `QCC_PR_ParseType` reads as a type there, a C type word in whatever
    /// case its keyword is found in (FTEF-20.14); `QCC_PR_ParseStatement_For` calls it, not the
    /// statement's own `STRCMP` tests.
    fn for_init_declares(&self) -> bool {
        // `QCC_PR_ParseStatement_For` reads the first clause with `QCC_PR_ParseType(false,
        // true, false)`, so any type starts a declaration there (FTEF-16.9).
        self.fte_looks_like_local() || self.c_type_word(0) || self.type_form(0)
    }

    /// One expression of a `for` clause: in the first clause each is discarded on its own
    /// (FTEF-18.25), and the value returned is only for the other clauses.
    fn for_expression(&mut self, clause: Clause) -> Option<Expr> {
        if clause == Clause::Init {
            self.discarded(Self::expression);
            None
        } else {
            self.expression()
        }
    }

    /// A `for` init declaration `type name = value` (FTE-2.2), without the trailing `;`.
    fn for_local_decl(&mut self, keyword: bool) {
        self.start_node(K::LocalDecl);
        if keyword {
            self.bump_as(K::LocalKw);
        }
        if let Some(ty) = self.type_or_user_type() {
            let mut first = true;
            loop {
                self.declarator(ty, Site::Body, first);
                first = false;
                if !self.at(K::Comma) {
                    break;
                }
                self.bump();
            }
        }
        self.finish_node();
    }

    /// `switch (e) { labels and statements }` (FTE-3).
    fn switch_stmt(&mut self) {
        let span = self.current_span();
        if !self.fteqcc() {
            self.extension("switch", span);
        }
        self.start_node(K::SwitchStmt);
        self.bump_as(K::SwitchKw);
        self.condition();
        if self.at(K::LBrace) || !self.fteqcc() {
            self.switch_body();
        } else {
            // FTEF-18.33: `QCC_PR_ParseStatement ();` reads any statement as the switch's body.
            self.statement();
        }
        self.finish_node();
    }

    /// The `{ ... }` of a `switch`: `case e:` and `default:` labels and the statements
    /// between them (FTE-3.1 to FTE-3.3).
    fn switch_body(&mut self) {
        self.start_node(K::Block);
        if !self.expect(K::LBrace, "{") {
            self.finish_node();
            return;
        }
        loop {
            let Some(kind) = self.current().map(|t| t.kind) else {
                self.error(Code::E0201, "not a name");
                break;
            };
            if kind == K::RBrace {
                self.bump();
                break;
            }
            if self.ends_at_declaration_line() {
                self.expected("}");
                break;
            }
            if self.at_case_label() {
                self.case_label();
                continue;
            }
            self.start_item();
            let before = self.pos;
            self.statement();
            if self.pos == before {
                self.bump_error();
            }
        }
        self.finish_node();
    }

    /// `case e:` or `default:` (FTE-3.1).
    fn case_label(&mut self) {
        self.start_item();
        self.start_node(K::CaseLabel);
        if self.kw(0, Keyword::Default, "default") {
            self.bump_as(K::DefaultKw);
        } else {
            self.bump_as(K::CaseKw);
            self.expression();
            if self.fteqcc() && self.at(K::DotDot) {
                // FTEF-18.10: `case a .. b:`, a range.
                self.bump();
                self.expression();
            }
        }
        self.expect(K::Colon, ":");
        self.finish_node();
    }

    /// `break;` or `continue;` (FTE-4).
    fn break_continue_stmt(&mut self, node: K, keyword: K, name: &str) {
        let span = self.current_span();
        if !self.fteqcc() {
            self.extension(name, span);
        }
        self.start_node(node);
        self.bump_as(keyword);
        self.end_statement();
        self.finish_node();
    }

    /// `local type name [= init] {, ...} ;` (QC-11.10, QC-11.9). Without `local`, the
    /// declaration is an error; it is parsed the same way so the tree keeps its shape. In an
    /// FTEQCC dialect a local needs no `local` (FTE-21.1), may have a user type (FTE-14.2) or
    /// the `const`/`var`/`nosave` qualifiers (FTE-10 to FTE-12), and may be a field or a
    /// function variable (FTE-21.2).
    fn local_decl(&mut self, keyword: bool) {
        self.start_node(K::LocalDecl);
        if keyword {
            self.bump_as(K::LocalKw);
        }
        if self.fteqcc() {
            // FTE-21.1: FTEQCC needs no `local`, for a field local `.float f;` too, and
            // `QCC_PR_ParseDefs` reads the qualifiers (FTE-10 to FTE-12, FTEF-15.1) with or
            // without it. FTEF-19.21: after a qualifier the type may be left out, an `int`.
            if self.fte_qualifiers() && self.fte_type_omitted() {
                self.declarators(TypeShape::default(), Site::Body);
                self.finish_node();
                return;
            }
        } else if !keyword {
            if self.at(K::Dot) {
                self.error(Code::E0201, "not a name");
            } else if let Some((text, span)) = self.nth(1).map(|t| (t.text.clone(), t.span)) {
                self.error_at(Code::E0204, format!("expected ;, found {text}"), span);
            }
        }
        let start = self.current_span().start;
        let ty = if self.fteqcc() {
            self.type_or_user_type()
        } else {
            self.type_()
        };
        match ty {
            Some(ty) => {
                // FTE-21.2: FTEQCC has no "Fields and functions must be global" check (it is
                // commented out of `QCC_PR_ParseDefs`), so a local may be a field or a
                // function variable.
                if (ty.field || ty.function) && !self.fteqcc() {
                    let span = Span::new(start, self.last_end);
                    self.error_at(Code::E0208, "Fields and functions must be global", span);
                }
                self.declarators(ty, Site::Body);
            }
            None => self.recover(true),
        }
        self.finish_node();
    }

    /// `return;`, `return e;`, and under FTEQCC `return = e;` (FTEF-18.7).
    fn return_stmt(&mut self) {
        self.start_node(K::ReturnStmt);
        self.bump_as(K::ReturnKw);
        if self.at(K::Semicolon) {
            self.bump();
        } else {
            if self.fteqcc() && self.at(K::Eq) {
                self.bump();
            }
            self.sequence();
            self.end_statement();
        }
        self.finish_node();
    }

    /// `while (e) s`, or under FTEQCC `__until (e) s` (FTEF-18.31).
    fn while_stmt(&mut self) {
        self.start_node(K::WhileStmt);
        if self.checks(0, Enabled::Always, "while") {
            self.bump_as(K::WhileKw);
        } else {
            self.bump_as(K::UntilKw);
        }
        self.condition();
        self.statement();
        self.finish_node();
    }

    fn do_stmt(&mut self) {
        self.start_node(K::DoStmt);
        self.bump_as(K::DoKw);
        self.statement();
        // FTEF-18.31, FTEF-20.5: `do s until (e);`, `until` with its keyword first, then
        // `QCC_PR_Expect ("while")`, which compares the token exactly.
        if self.fteqcc() && self.at_until() {
            self.bump_as(K::UntilKw);
        } else if self.at_word("while") {
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
        if self.fteqcc()
            && self.checks(0, Enabled::Always, "not")
            && self.nth(1).is_some_and(|t| t.kind == K::LParen)
        {
            // FTEF-16.5: `if not (e)` negates the condition.
            self.bump();
        } else if self.fteqcc()
            && self.at(K::Not)
            && self.nth(1).is_some_and(|t| t.kind == K::LParen)
        {
            // FTEF-18.8: `if !(e)` negates it too.
            self.bump();
        }
        self.condition();
        self.statement();
        if self.checks(0, Enabled::Always, "else") {
            self.bump_as(K::ElseKw);
            self.statement();
        }
        self.finish_node();
    }

    /// `( expression )`; the parentheses are required (QC-14.7). Under FTEQCC the expression
    /// may be a sequence (FTEF-18.5).
    fn condition(&mut self) {
        self.expect(K::LParen, "(");
        self.sequence();
        self.expect(K::RParen, ")");
    }

    /// `e;`, and under FTEQCC `e, e, ...;` (FTEF-18.5).
    fn expr_stmt(&mut self) {
        self.start_node(K::ExprStmt);
        self.discarded(Self::sequence);
        self.end_statement();
        self.finish_node();
    }

    fn end_statement(&mut self) {
        if !self.expect(K::Semicolon, ";") {
            self.recover(true);
        }
    }

    // ---- expressions ----

    /// `PR_Expression (TOP_PRIORITY)`. It is no operand of `!` or `~`, whatever it is in, so
    /// the operators it reads are not warned of (FTEF-21.6).
    fn expression(&mut self) -> Option<Expr> {
        let top = self.precedence().top();
        self.operand(top, false)
    }

    /// The operator priorities at the current token (FTEF-21.4): under an FTEQCC dialect those
    /// `QCC_PrioritiseOpcodes` set last, else QuakeC's.
    fn precedence(&self) -> Precedence {
        if self.fteqcc() {
            Precedence(self.flags_ahead(0).priorities())
        } else {
            Precedence(Priorities::Qc)
        }
    }

    /// An expression of priority `max` read as an operand of its own, `QCC_PR_Expression`'s
    /// with `EXPR_WARN_ABOVE_1` when `unary_not` (the operand of `!` or `~`, FTEF-21.6) and
    /// without it otherwise.
    fn operand(&mut self, max: u8, unary_not: bool) -> Option<Expr> {
        let outer = std::mem::replace(&mut self.unary_not, unary_not);
        let expr = self.expr(max);
        self.unary_not = outer;
        expr
    }

    /// An expression where FTEQCC allows a sequence, `a, b, c`, each evaluated and the last
    /// kept (FTEF-18.5): `QCC_PR_RefExpression` without `EXPR_DISALLOW_COMMA`. Each `,` is a
    /// `BinaryExpr`, grouped to the right. Vanilla has no sequence: there it is
    /// [`Parser::expression`].
    fn sequence(&mut self) -> Option<Expr> {
        let checkpoint = self.checkpoint();
        let first = self.expression()?;
        if !(self.fteqcc() && self.at(K::Comma)) {
            return Some(first);
        }
        self.discard_before_comma(&first);
        self.builder.start_node_at(checkpoint, K::BinaryExpr);
        self.bump();
        let rest = if self.enter() {
            self.sequence()
        } else {
            self.bump_error();
            None
        };
        self.leave(1);
        self.finish_node();
        Some(Expr {
            kind: K::BinaryExpr,
            assignable: false,
            span: Span::new(first.span.start, self.last_end),
            // The sequence is the reference of its last expression.
            tail: rest.map_or(Tail::Plain, |e| e.tail),
        })
    }

    /// The `,` of a sequence discards `expr` (FTEF-18.25): `QCC_PR_DiscardRef(lhsr); if
    /// (!qcc_usefulstatement) QCC_PR_ParseWarning(WARN_POINTLESSSTATEMENT, "Statement does
    /// not do anything"); qcc_usefulstatement = false;`.
    fn discard_before_comma(&mut self, expr: &Expr) {
        if expr.tail == Tail::PostInc {
            self.useful = true;
        }
        if !self.useful {
            self.pointless(expr.span);
        }
        self.useful = false;
    }

    /// An expression whose value is discarded, read by `parse`, as FTEQCC's
    /// `QCC_PR_DiscardExpression` reads an expression statement and a `for` clause
    /// (FTEF-18.25): the flag is cleared before and restored after, and the expression is
    /// "Statement does not do anything" when nothing set the flag since and it is not `void`.
    fn discarded(&mut self, parse: impl FnOnce(&mut Self) -> Option<Expr>) {
        let outer = std::mem::replace(&mut self.useful, false);
        if let Some(expr) = parse(self) {
            self.discard_at_end(&expr);
        }
        self.useful = outer;
    }

    /// The end of a discarded expression (FTEF-18.25): `QCC_PR_DiscardRef(ref); if
    /// (ref->cast->type != ev_void && !qcc_usefulstatement) QCC_PR_ParseWarning(...)`.
    fn discard_at_end(&mut self, expr: &Expr) {
        if expr.tail == Tail::PostInc {
            self.useful = true;
        }
        if self.fteqcc() && !self.useful && expr.tail != Tail::Void {
            self.pointless(expr.span);
        }
    }

    /// "Statement does not do anything" (FTEF-18.25).
    fn pointless(&mut self, span: Span) {
        self.warning_at(Code::W0313, "Statement does not do anything", span);
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
        while let Some(priority) = self.current().and_then(|t| self.operator_priority(t)) {
            if priority > max {
                break;
            }
            let precedence = self.precedence();
            let assign = self.current().map(|t| t.kind).is_some_and(is_assignment);
            levels = levels.saturating_add(1);
            if !self.enter() {
                self.bump_error();
                break;
            }
            // FTEF-16.10: FTEQCC's parser takes any left side; its checker warns of a
            // read-only one (FTEF-29.1).
            if assign && !lhs.assignable && !self.fteqcc() {
                self.error_at(Code::E0508, "cannot assign to this expression", lhs.span);
            }
            self.builder.start_node_at(checkpoint, K::BinaryExpr);
            // FTE-19.5: a negative number where an operator belongs is the operator and the
            // right operand at once, so it stays for the right operand to read.
            if self
                .current()
                .is_some_and(|t| precedence.binary(t.kind).is_some())
            {
                self.bump();
            }
            let rhs_max = if priority == precedence.assign() {
                priority
            } else {
                priority.saturating_sub(1)
            };
            let rhs = self.expr(rhs_max);
            // FTEF-21.6: `if (priority > 1 && exprflags & EXPR_WARN_ABOVE_1)
            // QCC_PR_ParseWarning(WARN_UNARYNOTSCOPE, ...)`, once the right operand is read.
            if self.unary_not && self.fteqcc() {
                let span = self.current_span();
                self.warning_at(
                    Code::W0227,
                    "suggest parenthesis for unary operator that applies to multiple terms",
                    span,
                );
            }
            if assign {
                // FTEF-18.25: an assignment sets `qcc_usefulstatement` once its right side is
                // read.
                self.useful = true;
            }
            self.finish_node();
            lhs = Expr {
                kind: K::BinaryExpr,
                assignable: false,
                span: Span::new(lhs.span.start, self.last_end),
                tail: Tail::Plain,
            };
            if rhs.is_none() {
                break;
            }
        }
        self.leave(levels);
        // FTE-8.4: the FTEQCC ternary `cond ? a : b` is read at `TERNARY_PRIORITY`, so its
        // condition is what was parsed so far at a tighter priority: `x = a ? b : c` assigns
        // the conditional, and `a && b ? c : d` is `a && (b ? c : d)`, since the right operand
        // of `=`, `&&` and `||` is read at this priority and reaches the `?` first. It is only
        // reachable in an FTEQCC dialect, since the vanilla lexer has no `?` token (it is
        // "Unknown punctuation").
        if max >= self.precedence().ternary() && self.at(K::Question) {
            lhs = self.ternary(checkpoint, lhs);
        }
        Some(lhs)
    }

    /// The priority of `tok` as a binary operator after an operand. In an FTEQCC dialect a
    /// negative number literal there is also one (FTE-19.5): FTEQCC's `QCC_PR_RefExpression`
    /// "work[s] around (4-3) being parsed as 4 -3 with no operator between" by reading it as
    /// `+` and the negative number, at the priority of `+`.
    fn operator_priority(&self, tok: &Tok) -> Option<u8> {
        let precedence = self.precedence();
        precedence.binary(tok.kind).or_else(|| {
            let negative = tok.kind == K::FloatLit && tok.text.starts_with('-');
            (self.fteqcc() && negative).then_some(precedence.add())
        })
    }

    /// `cond ? a : b` (FTE-8): the already-parsed `cond` is `lhs`; parse the two arms.
    fn ternary(&mut self, checkpoint: Checkpoint, cond: Expr) -> Expr {
        self.builder.start_node_at(checkpoint, K::TernaryExpr);
        self.bump_as(K::Question);
        if self.at(K::Colon) {
            // FTEF-18.11: `a ?: b` has no middle operand.
            self.bump();
        } else {
            // FTEF-18.5: the middle operand may be a sequence.
            self.sequence();
            self.expect(K::Colon, ":");
        }
        self.expression();
        self.finish_node();
        Expr {
            kind: K::TernaryExpr,
            assignable: false,
            span: Span::new(cond.span.start, self.last_end),
            tail: Tail::Plain,
        }
    }

    /// A term followed by field accesses, array subscripts (FTE-13) or postfix `++`/`--`
    /// (FTE-5), or by one call, which ends it (QC-15.8).
    fn postfix(&mut self, checkpoint: Checkpoint) -> Option<Expr> {
        let mut expr = self.term()?;
        let mut levels = 0_usize;
        loop {
            let call = self.at(K::LParen);
            // The FTEQCC postfix forms are only reachable in an FTEQCC dialect: the `[`
            // subscript, the `++`/`--` tokens, and the `->` member operator the vanilla lexer
            // never produces.
            let index = self.fteqcc() && self.at(K::LBracket);
            let incdec = self.at(K::PlusPlus) || self.at(K::MinusMinus);
            let member = self.at(K::Dot) || self.at(K::Arrow);
            if !call && !member && !index && !incdec {
                break;
            }
            levels = levels.saturating_add(1);
            if !self.enter() {
                self.bump_error();
                break;
            }
            let start = expr.span.start;
            if incdec {
                // `e++` / `e--` (FTE-5): a postfix of the lvalue, read then stored back.
                self.builder.start_node_at(checkpoint, K::PostfixExpr);
                self.bump();
                self.finish_node();
                expr = Expr {
                    kind: K::PostfixExpr,
                    assignable: false,
                    span: Span::new(start, self.last_end),
                    tail: Tail::PostInc,
                };
                continue;
            }
            if index {
                // `e[i]` (FTE-13): a subscript; the result is an lvalue.
                self.builder.start_node_at(checkpoint, K::IndexExpr);
                self.bump_as(K::LBracket);
                self.sequence();
                self.expect(K::RBracket, "]");
                self.finish_node();
                expr = Expr {
                    kind: K::IndexExpr,
                    assignable: true,
                    span: Span::new(start, self.last_end),
                    tail: Tail::Plain,
                };
                continue;
            }
            if call {
                // FTEF-18.25: `if (priority == FUNC_PRIORITY && QCC_PR_CheckToken ("(") ) {
                // qcc_usefulstatement=true;`
                self.useful = true;
                self.builder.start_node_at(checkpoint, K::CallExpr);
                self.arg_list();
                self.finish_node();
                expr = Expr {
                    kind: K::CallExpr,
                    assignable: false,
                    span: Span::new(start, self.last_end),
                    tail: Tail::Plain,
                };
                // FTEF-16.6: in FTEQCC a call's result may be indexed, called or have a
                // member selected; in vanilla a call ends the term (QC-15.8).
                if self.fteqcc() {
                    continue;
                }
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
                tail: Tail::Plain,
            };
            if field.is_none() {
                break;
            }
        }
        self.leave(levels);
        Some(expr)
    }

    /// `( args )` (QC-18.8).
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

    /// Whether the token at byte `at` was lexed with `flag` on (FTEF-12.12).
    fn lexed_with(&self, at: usize, flag: LexFlag) -> bool {
        qc_lexer::LexFlags::at(&self.input.flags, at).has(flag)
    }

    /// `PR_Term`: `!` expression, `( expression )`, a literal, a frame macro reference or a
    /// name (QC-15.7). Anything else is "not a name" (QC-4.5, QC-14.2).
    fn term(&mut self) -> Option<Expr> {
        let Some(tok) = self.current() else {
            self.error(Code::E0201, "not a name");
            return None;
        };
        let (kind, start, long) = (tok.kind, tok.span.start, self.too_long(&tok.text));
        let mut tail = Tail::Plain;
        let (node, assignable) = match kind {
            K::Not => {
                let not = self.precedence().not();
                self.start_node(K::NotExpr);
                self.bump();
                self.operand(not, true);
                (K::NotExpr, false)
            }
            // FTE-19.4: FTEQCC's `QCC_PR_RefTerm` reads `~` with an operand of `NOT_PRIORITY`,
            // as `!`, and `-` and `+` with one of `UNARY_PRIORITY`.
            K::Tilde | K::Minus | K::Plus if self.fteqcc() => {
                let max = if kind == K::Tilde {
                    self.precedence().not()
                } else {
                    UNARY_PRIORITY
                };
                self.start_node(K::UnaryExpr);
                self.bump();
                self.operand(max, kind == K::Tilde);
                (K::UnaryExpr, false)
            }
            // FTEF-16.2: the full dialect's casts, to any type.
            K::LParen if self.fteqcc() && self.fte_at_cast() => {
                // FTEF-18.25: `(void)` makes the expression `void`.
                let void_cast = self.type_word(1) == Some(K::VoidKw)
                    && self.nth(2).is_some_and(|t| t.kind == K::RParen);
                self.fte_cast();
                if void_cast {
                    tail = Tail::Void;
                }
                (K::CastExpr, false)
            }
            // FTEF-19.26: under `qccx`, `@e` is `e` read as a string (`QCC_PR_RefTerm`, `if
            // (flag_qccx && QCC_PR_CheckToken ("@"))`), its operand of `UNARY_PRIORITY`.
            K::At if self.fteqcc() && self.lexed_with(start, LexFlag::Qccx) => {
                self.start_node(K::UnaryExpr);
                self.bump();
                self.operand(UNARY_PRIORITY, false);
                (K::UnaryExpr, true)
            }
            // FTEF-16.7: `++e` and `--e`, `&e` and `*e`.
            K::PlusPlus | K::MinusMinus | K::Amp | K::Star if self.fteqcc() => {
                // FTEF-18.25: `if (preinc) { ... qcc_usefulstatement=true;`
                if matches!(kind, K::PlusPlus | K::MinusMinus) {
                    self.useful = true;
                }
                self.start_node(K::UnaryExpr);
                self.bump();
                self.operand(UNARY_PRIORITY, false);
                (K::UnaryExpr, kind == K::Star)
            }
            // FTEF-16.3: `[x, y, z]`.
            K::LBracket if self.fteqcc() => {
                self.vector_expr();
                (K::VectorExpr, false)
            }
            K::LParen => {
                self.start_node(K::ParenExpr);
                self.bump();
                let inner = self.sequence();
                self.expect(K::RParen, ")");
                // A parenthesised expression is the reference inside it.
                tail = inner.as_ref().map_or(Tail::Plain, |e| e.tail);
                (K::ParenExpr, inner.is_some_and(|e| e.assignable))
            }
            K::FloatLit | K::VectorLit | K::StringLit | K::FrameRef => {
                self.start_node(K::Literal);
                self.bump();
                // FTEF-16.8: adjacent string literals are one string.
                while self.fteqcc() && kind == K::StringLit && self.at(K::StringLit) {
                    self.bump();
                }
                (K::Literal, false)
            }
            // FTEF-16.4: `::name`, a name of the global scope.
            K::ColonColon if self.fteqcc() && self.nth(1).is_some_and(|t| t.kind == K::Name) => {
                self.start_node(K::NameRef);
                self.bump();
                self.bump();
                (K::NameRef, true)
            }
            // FTEF-18.12: `sizeof` and a type or an expression.
            K::Name if self.fteqcc() && self.checks(0, Enabled::Always, "sizeof") => {
                self.sizeof_expr();
                (K::SizeofExpr, false)
            }
            // FTEF-16.4: `Class::member` and `super::member`; a vanilla name has no `::`,
            // which only the FTEQCC lexer makes.
            K::Name => {
                self.scoped_name(long);
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
            tail,
        })
    }
}
