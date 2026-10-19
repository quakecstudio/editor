// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask fteqcc-coverage <dir>...`: the coverage check for the FTEQCC-rerelease dialect
//! spec (P2-01, FTE-1). It parses every `.qc` and `.qh` file under each rerelease codebase
//! directory with the vanilla parser (`qc-syntax`, `docs/spec/language.md`) and lists every
//! construct the vanilla grammar rejects, grouped into the classes of
//! `docs/spec/dialect-fteqcc-rerelease.md`. It fails if the source holds a rejected construct
//! that no class names, so the spec and the rerelease source cannot drift apart.
//!
//! The codebases come from `cargo xtask corpus fetch rerelease rerelease-hipnotic
//! rerelease-rogue rerelease-mg1 rerelease-ctf rerelease-mg3`. The recorded run over the six
//! is `docs/spec/vectors/dialect-fteqcc-rerelease/coverage.txt`.
//!
//! Detection works on the vanilla token stream, not on the recovery diagnostics, so it is
//! stable: each class has a signature of tokens, of the parentheses and braces around them,
//! or of an "Unknown punctuation" or "Unknown escape char" diagnostic for what vanilla `qcc`
//! has no token for. The classes cover FTE-2 to FTE-21 (P2-01, P2-33). A construct whose
//! syntax the vanilla parser accepts but whose meaning differs (the vector members of FTE-9:
//! `v.z` lexes as field access) is a semantic difference, not a parser rejection, and FTE-22
//! is name resolution, so both are documented in the spec but not listed here. The hanging
//! `;` of FTE-21.5 appears only after expansion: it is found as a use, followed by `;`, of a
//! macro whose body ends in `;`, which any file of the codebase may define.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::{Punct, Token, TokenKind, lex};
use qc_syntax::parse;

/// Which non-vanilla construct class a token or diagnostic belongs to. Each maps to one
/// section of `docs/spec/dialect-fteqcc-rerelease.md` the vanilla *parser* rejects (so not
/// FTE-1, which is about the progs format, FTE-9, a semantic difference, nor FTE-22, the name
/// resolution rules, which have no syntax of their own). Every construct found in the
/// rerelease source must be one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    For,
    Switch,
    Case,
    BreakContinue,
    IncDec,
    CompoundAssign,
    Modulo,
    Ternary,
    Const,
    Var,
    Nosave,
    Array,
    TypedefStruct,
    Enum,
    NamedEnum,
    Pragma,
    CStyleFunction,
    BodyWithoutEq,
    NamedBuiltin,
    Varargs,
    DefaultParam,
    NamedTypeParam,
    BitOperator,
    RemappedOperator,
    UnaryOperator,
    NegativeOperand,
    Cast,
    DotNumber,
    FloatSuffix,
    Escape,
    BareLocal,
    FieldOrFunctionLocal,
    MissingSemicolon,
    EmptyBlock,
    HangingSemicolon,
}

impl Kind {
    /// Every class, for the "every class is exercised" check.
    const ALL: [Self; 35] = [
        Self::For,
        Self::Switch,
        Self::Case,
        Self::BreakContinue,
        Self::IncDec,
        Self::CompoundAssign,
        Self::Modulo,
        Self::Ternary,
        Self::Const,
        Self::Var,
        Self::Nosave,
        Self::Array,
        Self::TypedefStruct,
        Self::Enum,
        Self::NamedEnum,
        Self::Pragma,
        Self::CStyleFunction,
        Self::BodyWithoutEq,
        Self::NamedBuiltin,
        Self::Varargs,
        Self::DefaultParam,
        Self::NamedTypeParam,
        Self::BitOperator,
        Self::RemappedOperator,
        Self::UnaryOperator,
        Self::NegativeOperand,
        Self::Cast,
        Self::DotNumber,
        Self::FloatSuffix,
        Self::Escape,
        Self::BareLocal,
        Self::FieldOrFunctionLocal,
        Self::MissingSemicolon,
        Self::EmptyBlock,
        Self::HangingSemicolon,
    ];

    /// The spec section that documents this class.
    const fn section(self) -> &'static str {
        match self {
            Self::For => "FTE-2",
            Self::Switch | Self::Case => "FTE-3",
            Self::BreakContinue => "FTE-4",
            Self::IncDec => "FTE-5",
            Self::CompoundAssign => "FTE-6",
            Self::Modulo => "FTE-7",
            Self::Ternary => "FTE-8",
            Self::Const => "FTE-10",
            Self::Var => "FTE-11",
            Self::Nosave => "FTE-12",
            Self::Array => "FTE-13",
            Self::TypedefStruct => "FTE-14",
            Self::Enum | Self::NamedEnum => "FTE-15",
            Self::Pragma => "FTE-16",
            Self::CStyleFunction | Self::BodyWithoutEq | Self::NamedBuiltin => "FTE-17",
            Self::Varargs | Self::DefaultParam | Self::NamedTypeParam => "FTE-18",
            Self::BitOperator
            | Self::RemappedOperator
            | Self::UnaryOperator
            | Self::NegativeOperand
            | Self::Cast => "FTE-19",
            Self::DotNumber | Self::FloatSuffix | Self::Escape => "FTE-20",
            Self::BareLocal
            | Self::FieldOrFunctionLocal
            | Self::MissingSemicolon
            | Self::EmptyBlock
            | Self::HangingSemicolon => "FTE-21",
        }
    }

    /// Whether the construct is made by macro expansion, so a file holding it may parse as
    /// written: a macro whose body ends in `;` used with a `;` after it (FTE-21.5).
    const fn after_expansion(self) -> bool {
        matches!(self, Self::HangingSemicolon)
    }

    /// A short name for the report.
    const fn name(self) -> &'static str {
        match self {
            Self::For => "for loop",
            Self::Switch => "switch",
            Self::Case => "case/default",
            Self::BreakContinue => "break/continue",
            Self::IncDec => "++/--",
            Self::CompoundAssign => "compound assignment",
            Self::Modulo => "% operator",
            Self::Ternary => "?: operator",
            Self::Const => "const",
            Self::Var => "var",
            Self::Nosave => "nosave",
            Self::Array => "array",
            Self::TypedefStruct => "typedef/struct",
            Self::Enum => "enum",
            Self::NamedEnum => "named enum type",
            Self::Pragma => "#pragma",
            Self::CStyleFunction => "C-style function",
            Self::BodyWithoutEq => "body without =",
            Self::NamedBuiltin => "named builtin",
            Self::Varargs => "... after parameters",
            Self::DefaultParam => "default parameter",
            Self::NamedTypeParam => "named-type parameter",
            Self::BitOperator => "bitwise operator",
            Self::RemappedOperator => "-> (+) (-)",
            Self::UnaryOperator => "unary - +",
            Self::NegativeOperand => "a -1",
            Self::Cast => "cast",
            Self::DotNumber => ".5 literal",
            Self::FloatSuffix => "f suffix",
            Self::Escape => "string escape",
            Self::BareLocal => "local without local",
            Self::FieldOrFunctionLocal => "field/function local",
            Self::MissingSemicolon => "missing ;",
            Self::EmptyBlock => "empty block",
            Self::HangingSemicolon => "hanging ;",
        }
    }
}

const USAGE: &str = "usage: cargo xtask fteqcc-coverage <dir>...";

/// Runs the check on the rerelease codebase directories in `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    let dirs: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if dirs.is_empty() {
        return Err(USAGE.to_owned());
    }
    let mut totals: BTreeMap<Kind, usize> = BTreeMap::new();
    let mut ok = true;
    for dir in &dirs {
        let files = sources(dir)?;
        if files.is_empty() {
            return Err(format!("no .qc or .qh files under {}", dir.display()));
        }
        let mut found: BTreeMap<Kind, usize> = BTreeMap::new();
        let mut rejects = 0_usize;
        let mut texts = Vec::with_capacity(files.len());
        for path in &files {
            texts.push(fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?);
        }
        // A macro defined in one file is used in others (FTE-21.5).
        let macros: BTreeSet<String> = texts.iter().flat_map(|t| semicolon_macros(t)).collect();
        for (path, text) in files.iter().zip(&texts) {
            let kinds = classify_with(text, &macros);
            // The vanilla parser must reject non-vanilla source: a file with a construct below
            // has at least one error. A file with a detected construct that the vanilla parser
            // accepts is a false positive in the detector and fails the check. A construct
            // that only the expansion shows is not in the file as the vanilla parser reads it.
            let vanilla_accepts = parse(text).errors().next().is_none();
            if vanilla_accepts && kinds.iter().any(|k| !k.after_expansion()) {
                ok = false;
                println!(
                    "FAIL  {}: a non-vanilla construct, but the vanilla parser accepts it",
                    path.display()
                );
            }
            for kind in kinds {
                let entry = found.entry(kind).or_default();
                *entry = entry.saturating_add(1);
                let total = totals.entry(kind).or_default();
                *total = total.saturating_add(1);
                rejects = rejects.saturating_add(1);
            }
        }
        println!(
            "fteqcc-coverage: {}: {} files, {rejects} non-vanilla constructs in {} classes",
            dir.display(),
            files.len(),
            found.len()
        );
        for (kind, count) in &found {
            println!("  {:<22} {count:>5}  {}", kind.name(), kind.section());
        }
    }
    println!("fteqcc-coverage: total non-vanilla constructs by class:");
    for (kind, count) in &totals {
        let present = in_spec(kind.section());
        println!(
            "  {:<22} {count:>5}  {} ({})",
            kind.name(),
            kind.section(),
            if present {
                "in spec"
            } else {
                "MISSING FROM SPEC"
            }
        );
        if !present {
            ok = false;
        }
    }
    // Every class the spec names must have been exercised, or the spec claims a construct the
    // rerelease does not use.
    for kind in Kind::ALL {
        if !totals.contains_key(&kind) {
            ok = false;
            println!(
                "FAIL  {} ({}) is in the spec but was not found in any codebase",
                kind.name(),
                kind.section()
            );
        }
    }
    println!(
        "fteqcc-coverage: {}",
        if ok {
            "every construct is in the spec"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// Whether the dialect spec names `section` as one of its sections.
fn in_spec(section: &str) -> bool {
    let spec =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/spec/dialect-fteqcc-rerelease.md");
    let Ok(text) = fs::read_to_string(spec) else {
        return false;
    };
    text.lines()
        .any(|line| line.starts_with(&format!("## {section} ")))
}

/// The classes of non-vanilla construct present in `text`, each once, with the macros whose
/// body ends in `;` those `text` defines.
#[cfg(test)]
fn classify(text: &[u8]) -> Vec<Kind> {
    classify_with(text, &semicolon_macros(text).into_iter().collect())
}

/// The names of the macros `text` defines whose body ends in `;` (QC-9.3): a use of one
/// followed by its own `;` is a hanging `;` once expanded (FTE-21.5).
fn semicolon_macros(text: &[u8]) -> Vec<String> {
    let tokens = lex(text).tokens;
    let mut names = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let define = index.checked_add(1).and_then(|i| tokens.get(i));
        let name = index.checked_add(2).and_then(|i| tokens.get(i));
        let (Some(define), Some(name)) = (define, name) else {
            continue;
        };
        let is_define = token.kind == TokenKind::Punct(Punct::Hash)
            && token.line_start
            && define.kind == TokenKind::Name("define".to_owned());
        let TokenKind::Name(macro_name) = &name.kind else {
            continue;
        };
        if !is_define {
            continue;
        }
        // The directive's last token: the last one before the next line break.
        let rest = tokens.get(index..).unwrap_or_default();
        let mut last = token;
        for pair in rest.windows(2) {
            let [a, b] = pair else { break };
            if text
                .get(a.span.end..b.span.start)
                .is_none_or(|between| between.contains(&b'\n'))
            {
                break;
            }
            last = b;
        }
        if last.kind == TokenKind::Punct(Punct::Semicolon) && !std::ptr::eq(last, name) {
            names.push(macro_name.clone());
        }
    }
    names
}

/// The classes of non-vanilla construct present in `text`, each once. `macros` are the names
/// of the macros whose body ends in `;` ([`semicolon_macros`]).
fn classify_with(text: &[u8], macros: &BTreeSet<String>) -> Vec<Kind> {
    let lexed = lex(text);
    let mut scan = Scan {
        text,
        tokens: &lexed.tokens,
        macros,
        kinds: Vec::new(),
        parens: 0,
        open: Vec::new(),
        closed_initialiser: false,
        open_local: false,
    };
    // `%`, `?`, `:`, `^` and `~` have no vanilla token; the lexer reports "Unknown
    // punctuation" (QC-7.2) and skips the byte. An escape vanilla lacks is "Unknown escape
    // char" (QC-6.2).
    for diagnostic in &lexed.diagnostics {
        if diagnostic.message == "Unknown escape char" {
            scan.add(Kind::Escape);
            continue;
        }
        match text.get(diagnostic.span.start) {
            Some(b'%') => scan.add(Kind::Modulo),
            Some(b'?') => scan.add(Kind::Ternary),
            Some(b':') => {
                if let Some(kind) = colon_kind(&lexed.tokens, diagnostic.span.start) {
                    scan.add(kind);
                }
            }
            Some(b'^' | b'~') => scan.add(Kind::BitOperator),
            _ => {}
        }
    }
    for index in 0..lexed.tokens.len() {
        scan.token(index);
    }
    scan.kinds
}

/// One token's neighbours and where it stands.
struct At<'a> {
    token: &'a Token,
    prev: Option<&'a Token>,
    prev2: Option<&'a Token>,
    next: Option<&'a Token>,
    next2: Option<&'a Token>,
    /// Inside a brace: a function body or an initialiser.
    body: bool,
    /// After a `;`, `{` or `}`, or first in the file.
    statement_start: bool,
}

/// The state of classifying one file's tokens.
struct Scan<'a> {
    text: &'a [u8],
    tokens: &'a [Token],
    macros: &'a BTreeSet<String>,
    kinds: Vec<Kind>,
    /// The open parentheses.
    parens: usize,
    /// The open braces, innermost last: whether each opened an initialiser `{ a, b }` in a
    /// body (FTE-13.1, FTE-14.3) rather than a block.
    open: Vec<bool>,
    /// Whether the last `}` closed an initialiser.
    closed_initialiser: bool,
    /// Whether a `local` declaration has started and not yet reached its `;`.
    open_local: bool,
}

impl<'a> Scan<'a> {
    fn add(&mut self, kind: Kind) {
        if !self.kinds.contains(&kind) {
            self.kinds.push(kind);
        }
    }

    /// Classifies the token at `index`, then tracks the parentheses and braces.
    fn token(&mut self, index: usize) {
        let tokens = self.tokens;
        let near = |offset: isize| index.checked_add_signed(offset).and_then(|i| tokens.get(i));
        let Some(token) = tokens.get(index) else {
            return;
        };
        let prev = near(-1);
        let at = At {
            token,
            prev,
            prev2: near(-2),
            next: near(1),
            next2: near(2),
            body: !self.open.is_empty(),
            statement_start: prev.is_none_or(|p| {
                matches!(
                    p.kind,
                    TokenKind::Punct(Punct::Semicolon | Punct::LBrace | Punct::RBrace)
                )
            }),
        };
        match &token.kind {
            TokenKind::Name(word) => self.name(&at, word),
            TokenKind::Punct(Punct::Dot) => self.dot(&at),
            TokenKind::Float(_) => self.float(&at),
            // A `[` after `=` is the vanilla state-function form `=[frame, think]` (QC-13.6),
            // which the vanilla parser accepts. An FTEQCC array declaration (`name[...]`) or
            // subscript (`name[i]`) has the `[` right after a name or a closing `]`.
            TokenKind::Punct(Punct::LBracket)
                if prev.is_some_and(|p| {
                    matches!(p.kind, TokenKind::Name(_))
                        || p.kind == TokenKind::Punct(Punct::RBracket)
                }) =>
            {
                self.add(Kind::Array);
            }
            TokenKind::Punct(op @ (Punct::Plus | Punct::Minus)) => self.plus_minus(&at, *op),
            TokenKind::Punct(Punct::Star | Punct::Slash) => {
                if touches_eq(token, at.next) {
                    self.add(Kind::CompoundAssign);
                }
            }
            TokenKind::Punct(op @ (Punct::Less | Punct::Greater | Punct::And | Punct::Or)) => {
                self.bit_operator(&at, *op);
            }
            TokenKind::Punct(Punct::LParen) => self.cast(&at),
            TokenKind::Punct(Punct::Eq) if !at.body && self.parens > 0 => {
                // FTE-18.2: `type name = value` in a parameter list.
                self.add(Kind::DefaultParam);
            }
            TokenKind::Punct(Punct::Ellipsis)
                if prev.is_some_and(|p| p.kind == TokenKind::Punct(Punct::Comma)) =>
            {
                // FTE-18.1: `...` after named parameters.
                self.add(Kind::Varargs);
            }
            TokenKind::Punct(Punct::LBrace) => self.left_brace(&at),
            TokenKind::Punct(Punct::Semicolon) => self.semicolon(&at),
            // A `#` at the start of a line begins a directive (QC-9.1). `#pragma` is the one
            // FTEQCC directive the rerelease uses (FTE-16); the vanilla directives
            // `#include`/`#define`/`#ifdef` and so on stay vanilla (QC-9). A `#` not at a line
            // start is the vanilla builtin marker `#6` (QC-13.3), and `#0` is an FTEQCC named
            // builtin's (FTE-17.4).
            TokenKind::Punct(Punct::Hash) => {
                if token.line_start
                    && at
                        .next
                        .is_some_and(|n| n.kind == TokenKind::Name("pragma".to_owned()))
                {
                    self.add(Kind::Pragma);
                } else if at.next.is_some_and(|n| n.kind == TokenKind::Float(0.0)) {
                    self.add(Kind::NamedBuiltin);
                }
            }
            _ => {}
        }
        match &token.kind {
            TokenKind::Punct(Punct::RBrace) => {
                self.closed_initialiser = self.open.pop().unwrap_or(false);
            }
            TokenKind::Punct(Punct::LParen) => self.parens = self.parens.saturating_add(1),
            TokenKind::Punct(Punct::RParen) => self.parens = self.parens.saturating_sub(1),
            _ => {}
        }
    }

    fn name(&mut self, at: &At<'a>, word: &str) {
        let token = at.token;
        // A keyword is a construct only where a declaration or statement starts: the first
        // token of its line (QC's keywords are contextual, QC-3.2). This skips a keyword used
        // as an identifier, such as the `var` parameter name in id's
        // `void(string var, string val) cvar_set = #72;` or the `break` builtin name
        // `void() break = #6;`.
        if token.line_start
            && let Some(kind) = keyword(word)
        {
            self.add(kind);
        }
        // FTE-15.4: `enum Name {`, `enum Name :` and `enum class Name` name a type;
        // `enum : int` does not (the lexer skips the `:`, so look at the bytes).
        let named = at.next.is_some_and(|n| {
            matches!(n.kind, TokenKind::Name(_))
                && self
                    .text
                    .get(token.span.end..n.span.start)
                    .is_some_and(|between| !between.contains(&b':'))
        });
        if word == "enum" && named {
            self.add(Kind::NamedEnum);
        }
        self.local(at, word);
        let next_is_name = at
            .next
            .is_some_and(|n| matches!(n.kind, TokenKind::Name(_)));
        let next2_opens = at
            .next2
            .is_some_and(|n| n.kind == TokenKind::Punct(Punct::LParen));
        if !at.body
            && is_type(&token.kind)
            && (at.statement_start || token.line_start)
            && next_is_name
            && next2_opens
        {
            // FTE-17.1: `rettype name(params)` at file level.
            self.add(Kind::CStyleFunction);
        }
        let in_list = at
            .prev
            .is_some_and(|p| matches!(p.kind, TokenKind::Punct(Punct::LParen | Punct::Comma)));
        if !at.body && self.parens > 0 && !is_type(&token.kind) && next_is_name && in_list {
            // FTE-18.3: a parameter of a type the program named.
            self.add(Kind::NamedTypeParam);
        }
        if at.body
            && self.macros.contains(word)
            && at
                .next
                .is_some_and(|n| n.kind == TokenKind::Punct(Punct::Semicolon))
        {
            // FTE-21.5: a macro whose body ends in `;`, used with a `;` after it.
            self.add(Kind::HangingSemicolon);
        }
        if at.body && self.parens == 0 && at.statement_start {
            let opens_list = at
                .next
                .is_some_and(|n| n.kind == TokenKind::Punct(Punct::LParen));
            if is_type(&token.kind) && opens_list {
                // FTE-21.2: a function local, `void() think;`, with no `local`.
                self.add(Kind::FieldOrFunctionLocal);
            } else if next_is_name && !is_statement_keyword(word) {
                // FTE-21.1: `float x` or `Type x` with no `local`.
                self.add(Kind::BareLocal);
            }
        }
    }

    /// `local` declarations: a field or function local (FTE-21.2), and a missing `;` (FTE-21.3).
    fn local(&mut self, at: &At<'a>, word: &str) {
        if word == "local" && at.body {
            self.open_local = true;
            // FTE-21.2: `local .T name` and `local T(...) name`.
            let field = at
                .next
                .is_some_and(|n| n.kind == TokenKind::Punct(Punct::Dot));
            let function = at.next.is_some_and(|n| is_type(&n.kind))
                && at
                    .next2
                    .is_some_and(|n| n.kind == TokenKind::Punct(Punct::LParen));
            if field || function {
                self.add(Kind::FieldOrFunctionLocal);
            }
        } else if self.open_local && at.token.line_start && at.prev.is_some_and(ends_value) {
            // FTE-21.3: a `local` whose line ends at a value and is followed by a new
            // statement, with no `;` between.
            self.open_local = false;
            self.add(Kind::MissingSemicolon);
        }
    }

    fn dot(&mut self, at: &At<'a>) {
        // FTE-20.1: `.5`, a `.` touching a number that starts with a digit.
        let number = at.next.is_some_and(|n| {
            matches!(n.kind, TokenKind::Float(_))
                && n.span.start == at.token.span.end
                && self.text.get(n.span.start).is_some_and(u8::is_ascii_digit)
        });
        if number {
            self.add(Kind::DotNumber);
        } else if at.body && self.parens == 0 && at.statement_start {
            // FTE-21.2: a field local, `.float f;`, with no `local`.
            self.add(Kind::FieldOrFunctionLocal);
        }
    }

    fn float(&mut self, at: &At<'a>) {
        // FTE-20.2: `1.0f`, the suffix lexed as a name touching the number.
        if at.next.is_some_and(|n| {
            matches!(&n.kind, TokenKind::Name(s) if s == "f" || s == "F")
                && n.span.start == at.token.span.end
        }) {
            self.add(Kind::FloatSuffix);
        }
        // FTE-19.5: `a -1`, a negative number where a binary operator belongs.
        if self.text.get(at.token.span.start) == Some(&b'-') && at.prev.is_some_and(ends_value) {
            self.add(Kind::NegativeOperand);
        }
    }

    fn plus_minus(&mut self, at: &At<'a>, op: Punct) {
        let token = at.token;
        let touching = |n: Option<&Token>, kind: TokenKind| {
            n.is_some_and(|n| n.kind == kind && n.span.start == token.span.end)
        };
        // `++` or `--`: two of the same operator whose spans touch.
        if touching(at.next, TokenKind::Punct(op)) {
            self.add(Kind::IncDec);
        }
        if touches_eq(token, at.next) {
            self.add(Kind::CompoundAssign);
        }
        let arrow = op == Punct::Minus && touching(at.next, TokenKind::Punct(Punct::Greater));
        let in_parens = at.prev.is_some_and(|p| {
            p.kind == TokenKind::Punct(Punct::LParen) && p.span.end == token.span.start
        }) && touching(at.next, TokenKind::Punct(Punct::RParen));
        let term_follows = at.next.is_some_and(|n| {
            matches!(n.kind, TokenKind::Name(_) | TokenKind::Punct(Punct::LParen))
        });
        if arrow || in_parens {
            // FTE-19.2: `->`, `(+)` and `(-)`.
            self.add(Kind::RemappedOperator);
        } else if at.prev.is_some_and(|p| !ends_value(p)) && term_follows {
            // FTE-19.4: `-e` and `+e`, an operator where a term starts.
            self.add(Kind::UnaryOperator);
        }
    }

    /// FTE-19.1: `<<`, `>>`, `&=`, `|=` and their compounds.
    fn bit_operator(&mut self, at: &At<'a>, op: Punct) {
        let token = at.token;
        let doubled = at.next.is_some_and(|n| {
            let same = if op == Punct::Less {
                matches!(n.kind, TokenKind::Punct(Punct::Less | Punct::LessEq))
            } else if op == Punct::Greater {
                matches!(n.kind, TokenKind::Punct(Punct::Greater | Punct::GreaterEq))
            } else {
                false
            };
            same && n.span.start == token.span.end
        });
        let assigning = matches!(op, Punct::And | Punct::Or) && touches_eq(token, at.next);
        // `->` is `-` and `>`; its `>` is not a shift.
        let after_minus = at.prev.is_some_and(|p| {
            p.kind == TokenKind::Punct(Punct::Minus) && p.span.end == token.span.start
        });
        if (doubled && !after_minus) || assigning {
            self.add(Kind::BitOperator);
        }
    }

    /// FTE-19.6: `(type) e`, a `(` opening a type where a term starts. A vanilla `(` before a
    /// type follows a type or a name: a function type's parameters.
    fn cast(&mut self, at: &At<'a>) {
        let cast_position = at.prev.is_some_and(|p| match &p.kind {
            TokenKind::Name(w) => w == "return",
            TokenKind::Punct(Punct::RParen | Punct::RBracket) => false,
            TokenKind::Punct(_) => true,
            TokenKind::Float(_)
            | TokenKind::Vector(_)
            | TokenKind::String(_)
            | TokenKind::FrameRef(_)
            | TokenKind::Frames(_) => false,
        });
        if cast_position && at.next.is_some_and(|n| is_type(&n.kind)) {
            self.add(Kind::Cast);
        }
    }

    fn left_brace(&mut self, at: &At<'a>) {
        let after_paren =
            |p: Option<&Token>| p.is_some_and(|p| p.kind == TokenKind::Punct(Punct::RParen));
        let named = at
            .prev
            .is_some_and(|p| matches!(p.kind, TokenKind::Name(_)))
            && after_paren(at.prev2);
        if !at.body && (after_paren(at.prev) || named) {
            // FTE-17.2: a function body with no `=` before it.
            self.add(Kind::BodyWithoutEq);
        }
        let initialiser = at.body
            && at.prev.is_some_and(|p| match p.kind {
                TokenKind::Punct(Punct::Eq | Punct::Comma) => true,
                TokenKind::Punct(Punct::LBrace) => self.open.last() == Some(&true),
                _ => false,
            });
        let empty = at
            .next
            .is_some_and(|n| n.kind == TokenKind::Punct(Punct::RBrace));
        if at.body && !initialiser && empty {
            // FTE-21.4: an empty nested block.
            self.add(Kind::EmptyBlock);
        }
        self.open.push(initialiser);
    }

    fn semicolon(&mut self, at: &At<'a>) {
        self.open_local = false;
        let after_initialiser = self.closed_initialiser
            && at
                .prev
                .is_some_and(|p| p.kind == TokenKind::Punct(Punct::RBrace));
        if at.body && self.parens == 0 && at.statement_start && !after_initialiser {
            // FTE-21.5: a `;` alone, after another statement's end.
            self.add(Kind::HangingSemicolon);
        }
    }
}

/// The class of a `:`, which vanilla has no token for: the name of a named builtin
/// `#0:name` (FTE-17.4), nothing in a `case` or `default` label (FTE-3) or after `enum`
/// (FTE-15.1), and otherwise the ternary's (FTE-8).
fn colon_kind(tokens: &[Token], offset: usize) -> Option<Kind> {
    let before = tokens.partition_point(|t| t.span.start < offset);
    let prev = before.checked_sub(1).and_then(|i| tokens.get(i));
    let prev2 = before.checked_sub(2).and_then(|i| tokens.get(i));
    if prev.is_some_and(|p| matches!(p.kind, TokenKind::Float(_)))
        && prev2.is_some_and(|p| p.kind == TokenKind::Punct(Punct::Hash))
    {
        return Some(Kind::NamedBuiltin);
    }
    // The first token of the colon's line, and whether the line names an enum.
    let line_first = tokens
        .get(..before)
        .unwrap_or_default()
        .iter()
        .rev()
        .find(|t| t.line_start);
    let labelled = line_first.is_some_and(
        |t| matches!(&t.kind, TokenKind::Name(w) if w == "case" || w == "default" || w == "enum"),
    );
    let enum_before = tokens
        .get(before.saturating_sub(3)..before)
        .unwrap_or_default()
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Name(w) if w == "enum" || w == "class"));
    if labelled || enum_before {
        return None;
    }
    Some(Kind::Ternary)
}

/// Whether a token is a vanilla type keyword (QC-10.1).
fn is_type(kind: &TokenKind) -> bool {
    matches!(kind, TokenKind::Name(w) if matches!(w.as_str(), "void" | "float" | "vector" | "string" | "entity"))
}

/// Whether a word begins a statement that is not a declaration, so `word name` is not a
/// local (QC-14, FTE-2 to FTE-4).
fn is_statement_keyword(word: &str) -> bool {
    matches!(
        word,
        "return"
            | "local"
            | "if"
            | "else"
            | "while"
            | "do"
            | "for"
            | "switch"
            | "case"
            | "default"
            | "break"
            | "continue"
            | "const"
            | "var"
            | "nosave"
            | "enum"
            | "typedef"
    )
}

/// Whether a token ends a value, so an operator after it is binary: a name other than a
/// statement keyword, a literal, a frame reference, or a closing `)` or `]`.
fn ends_value(token: &Token) -> bool {
    match &token.kind {
        TokenKind::Name(w) => !is_statement_keyword(w),
        TokenKind::Float(_)
        | TokenKind::Vector(_)
        | TokenKind::String(_)
        | TokenKind::FrameRef(_)
        | TokenKind::Punct(Punct::RParen | Punct::RBracket) => true,
        TokenKind::Punct(_) | TokenKind::Frames(_) => false,
    }
}

/// Whether `token` is immediately followed by `=` with no gap, so the pair is a
/// compound-assignment operator (`+=`, `-=`, `*=`, `/=`), not the operator then an `=`.
fn touches_eq(token: &Token, next: Option<&Token>) -> bool {
    next.is_some_and(|n| n.kind == TokenKind::Punct(Punct::Eq) && n.span.start == token.span.end)
}

/// The construct class a keyword name belongs to, if it is one the vanilla parser rejects in
/// the positions the rerelease uses it.
fn keyword(word: &str) -> Option<Kind> {
    match word {
        "for" => Some(Kind::For),
        "switch" => Some(Kind::Switch),
        "case" | "default" => Some(Kind::Case),
        "break" | "continue" => Some(Kind::BreakContinue),
        "const" => Some(Kind::Const),
        "var" => Some(Kind::Var),
        "nosave" => Some(Kind::Nosave),
        "enum" => Some(Kind::Enum),
        "typedef" | "struct" => Some(Kind::TypedefStruct),
        _ => None,
    }
}

/// Every `.qc` and `.qh` file under `dir`, sorted.
fn sources(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("qc") || x.eq_ignore_ascii_case("qh"))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{Kind, classify, in_spec, main};

    fn codebase(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/xtask-tests")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(dir.join("sub")).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    // spec FTE-2.1: `for (...)` is detected as a for loop.
    // spec FTE-3.1, FTE-3.2: `switch`/`case`/`default` are detected.
    // spec FTE-4.1: `break` and `continue` are detected.
    #[test]
    fn keywords_are_classified() {
        assert_eq!(classify(b"for (a) b;"), vec![Kind::For]);
        assert!(classify(b"switch (x) {}").contains(&Kind::Switch));
        assert!(classify(b"case A:\ndefault:").contains(&Kind::Case));
        assert!(classify(b"break;\ncontinue;").contains(&Kind::BreakContinue));
    }

    // spec FTE-3.1: a keyword used as an identifier mid-line is not a construct: the `break`
    // builtin name and the `var` parameter name id 1.01 and the rerelease both use.
    #[test]
    fn a_keyword_as_an_identifier_is_not_a_construct() {
        assert!(classify(b"void() break = #6;\n").is_empty());
        assert!(classify(b"void(string var, string val) cvar_set = #72;\n").is_empty());
    }

    // spec FTE-5.1: `i++`/`i--` are detected and `a + +b` is not.
    #[test]
    fn increment_needs_adjacent_operators() {
        assert!(classify(b"i++;").contains(&Kind::IncDec));
        assert!(classify(b"i--;").contains(&Kind::IncDec));
        assert!(!classify(b"a + + b;").contains(&Kind::IncDec));
    }

    // spec FTE-6.1: `x += 1` is a compound assignment and `x = +1` is not.
    #[test]
    fn compound_assignment_needs_a_touching_equals() {
        assert!(classify(b"x += 1;").contains(&Kind::CompoundAssign));
        assert!(classify(b"x *= 2;").contains(&Kind::CompoundAssign));
        assert!(classify(b"x /= 2;").contains(&Kind::CompoundAssign));
        assert!(!classify(b"x = +1;").contains(&Kind::CompoundAssign));
        assert!(!classify(b"x = + 1;").contains(&Kind::CompoundAssign));
    }

    // spec FTE-7.1: `%` is a modulo operator.
    // spec FTE-8.1: `?` and `:` are the ternary operator.
    #[test]
    fn unknown_punctuation_is_classified() {
        assert!(classify(b"a % b").contains(&Kind::Modulo));
        assert!(classify(b"a ? b : c").contains(&Kind::Ternary));
    }

    // spec FTE-10.1, FTE-11.1, FTE-12.1: const/var/nosave are detected.
    // spec FTE-13.1: an array subscript or declaration is detected.
    // spec FTE-14.1: typedef/struct are detected.
    // spec FTE-15.1: enum is detected.
    // spec FTE-16.1: #pragma is detected.
    #[test]
    fn declarations_and_pragma_are_classified() {
        assert!(classify(b"const float A = 1;").contains(&Kind::Const));
        assert!(classify(b"var float a;").contains(&Kind::Var));
        assert!(classify(b"nosave float a;").contains(&Kind::Nosave));
        assert!(classify(b"float a[3];").contains(&Kind::Array));
        assert!(classify(b"typedef struct { float f; } S;").contains(&Kind::TypedefStruct));
        assert!(classify(b"enum { A, B };").contains(&Kind::Enum));
        assert!(classify(b"#pragma warning disable F317").contains(&Kind::Pragma));
    }

    #[test]
    fn vanilla_source_has_no_construct() {
        assert!(classify(b"float a;\nvoid() f = { a = 1; };\n").is_empty());
    }

    #[test]
    fn the_spec_names_every_detected_section() {
        for kind in Kind::ALL {
            assert!(in_spec(kind.section()), "{}", kind.section());
        }
        assert!(!in_spec("FTE-99"));
    }

    // spec FTE-15.4: `enum Name` and `enum class Name` name a type; `enum : int` and a bare
    // `enum {` do not.
    #[test]
    fn a_named_enum_is_classified() {
        assert!(classify(b"enum UpgradeType { A };").contains(&Kind::NamedEnum));
        assert!(classify(b"enum class Code : int { A };").contains(&Kind::NamedEnum));
        assert!(!classify(b"enum : int { A };").contains(&Kind::NamedEnum));
        assert!(!classify(b"enum { A };").contains(&Kind::NamedEnum));
        // The `:` of a typed enum is not a ternary's.
        assert!(!classify(b"enum : int { A };").contains(&Kind::Ternary));
    }

    // spec FTE-17.1: `rettype name(params)` at file level is a C-style declaration; the vanilla
    // `rettype(params) name` and a call in a body are not.
    #[test]
    fn a_c_style_function_is_classified() {
        assert!(classify(b"void makevectorsfixed(vector ang);").contains(&Kind::CStyleFunction));
        assert!(!classify(b"void(vector ang) makevectorsfixed;").contains(&Kind::CStyleFunction));
        assert!(
            !classify(b"void() f = {\nfloat x;\nx = g(1);\n};").contains(&Kind::CStyleFunction)
        );
    }

    // spec FTE-17.2: a body after `)` or after a vanilla type's name, with no `=`; a vanilla
    // `= {` body and a `typedef struct {` are not.
    #[test]
    fn a_body_without_eq_is_classified() {
        assert!(classify(b"void Init() { };").contains(&Kind::BodyWithoutEq));
        assert!(classify(b"void(entity e) remove { };").contains(&Kind::BodyWithoutEq));
        assert!(!classify(b"void() f = { };").contains(&Kind::BodyWithoutEq));
        assert!(!classify(b"typedef struct { float f; } S;").contains(&Kind::BodyWithoutEq));
    }

    // spec FTE-17.4: a builtin numbered 0 or bound by name; its `:` is not a ternary's, and a
    // vanilla `#6` is not a named builtin.
    #[test]
    fn a_named_builtin_is_classified() {
        let named = classify(b"void(string s) print = #1:ex_print;");
        assert!(named.contains(&Kind::NamedBuiltin));
        assert!(!named.contains(&Kind::Ternary));
        assert!(classify(b"void() f = #0;").contains(&Kind::NamedBuiltin));
        assert!(!classify(b"void() break = #6;").contains(&Kind::NamedBuiltin));
    }

    // spec FTE-18.1: `...` after named parameters; a whole-list `(...)` is vanilla (QC-10.4).
    #[test]
    fn varargs_after_parameters_are_classified() {
        assert!(classify(b"void(string s, ...) bprint;").contains(&Kind::Varargs));
        assert!(!classify(b"void(...) bprint;").contains(&Kind::Varargs));
    }

    // spec FTE-18.2: `type name = value` in a parameter list; an `=` after the list is the
    // vanilla initialiser.
    #[test]
    fn a_default_parameter_is_classified() {
        assert!(classify(b"void(float a = 0) f;").contains(&Kind::DefaultParam));
        assert!(!classify(b"void(float a) f = #1;").contains(&Kind::DefaultParam));
    }

    // spec FTE-18.3: a parameter of a type the program named; a vanilla parameter is a type
    // keyword and a name.
    #[test]
    fn a_named_type_parameter_is_classified() {
        assert!(classify(b"float GetAmount(UpgradeType Type);").contains(&Kind::NamedTypeParam));
        assert!(!classify(b"void(float a, entity b) f;").contains(&Kind::NamedTypeParam));
    }

    // spec FTE-19.1: `^`, `~`, `<<`, `>>`, `&=`, `|=` and `&~=` are FTEQCC's further operators;
    // `<`, `<=` and the `>` of `->` are not.
    #[test]
    fn bitwise_operators_are_classified() {
        for source in [
            "a = b ^ c;",
            "a = ~b;",
            "a = b << c;",
            "a = b >> c;",
            "a &= b;",
            "a |= b;",
            "a &~= b;",
            "a <<= b;",
        ] {
            assert!(
                classify(source.as_bytes()).contains(&Kind::BitOperator),
                "{source}"
            );
        }
        for source in ["a = b < c;", "a = b <= c;", "a = e->f;", "a = b & c;"] {
            assert!(
                !classify(source.as_bytes()).contains(&Kind::BitOperator),
                "{source}"
            );
        }
    }

    // spec FTE-19.2: `->`, `(+)` and `(-)` are read as `.`, `|=` and `&~=`; `(-1)` is not.
    #[test]
    fn remapped_operators_are_classified() {
        for source in ["a = e->f;", "a (+) b;", "a (-) b;"] {
            assert!(
                classify(source.as_bytes()).contains(&Kind::RemappedOperator),
                "{source}"
            );
        }
        assert!(!classify(b"a = f(-1);").contains(&Kind::RemappedOperator));
        assert!(!classify(b"a = b - c;").contains(&Kind::RemappedOperator));
    }

    // spec FTE-19.4: `-e` and `+e` where a term starts; a binary `-` is not.
    #[test]
    fn unary_operators_are_classified() {
        assert!(classify(b"a = -fabs(b);").contains(&Kind::UnaryOperator));
        assert!(classify(b"a = +b;").contains(&Kind::UnaryOperator));
        assert!(classify(b"a = (-b);").contains(&Kind::UnaryOperator));
        assert!(!classify(b"a = b - c;").contains(&Kind::UnaryOperator));
        assert!(!classify(b"a = b() - c;").contains(&Kind::UnaryOperator));
    }

    // spec FTE-19.5: a negative number after a value is a subtraction; after an operator or
    // `return` it is a vanilla negative literal.
    #[test]
    fn a_negative_operand_is_classified() {
        assert!(classify(b"a = self.count -1;").contains(&Kind::NegativeOperand));
        assert!(classify(b"a = f() -1;").contains(&Kind::NegativeOperand));
        assert!(!classify(b"a = -1;").contains(&Kind::NegativeOperand));
        assert!(!classify(b"return -1;").contains(&Kind::NegativeOperand));
    }

    // spec FTE-19.6: `(type) e` where a term starts; a function type's parameter list is not a
    // cast.
    #[test]
    fn a_cast_is_classified() {
        assert!(classify(b"a = (float) b;").contains(&Kind::Cast));
        assert!(classify(b"void(float(entity e) p = (float(entity e))T) f;").contains(&Kind::Cast));
        assert!(classify(b"return (float) b;").contains(&Kind::Cast));
        assert!(!classify(b"void(float a) f;").contains(&Kind::Cast));
        assert!(!classify(b"a = g(b) (c);").contains(&Kind::Cast));
    }

    // spec FTE-20.1: `.5`, a `.` touching a number; a field access `e.f` is not.
    // spec FTE-20.2: `1.0f`, a float with its suffix; `1 f` is not.
    // spec FTE-20.3: an escape vanilla lacks.
    #[test]
    fn further_literals_are_classified() {
        assert!(classify(b"float a = .5;").contains(&Kind::DotNumber));
        assert!(!classify(b"a = e.f;").contains(&Kind::DotNumber));
        assert!(classify(b"a = 1.0f;").contains(&Kind::FloatSuffix));
        assert!(!classify(b"a = 1 f;").contains(&Kind::FloatSuffix));
        assert!(classify(b"string s = \"\\t\";").contains(&Kind::Escape));
        assert!(!classify(b"string s = \"\\n\";").contains(&Kind::Escape));
    }

    // spec FTE-21.1: a typed local with no `local`, of a vanilla or a named type, where a
    // statement starts; `return x` and `local float x` are not.
    #[test]
    fn a_local_without_local_is_classified() {
        assert!(classify(b"void() f = {\nfloat x = 1;\n};").contains(&Kind::BareLocal));
        assert!(classify(b"void() f = { a = 1; poseInfo p; };").contains(&Kind::BareLocal));
        assert!(!classify(b"float() f = {\nreturn x;\n};").contains(&Kind::BareLocal));
        assert!(!classify(b"void() f = {\nlocal float x;\n};").contains(&Kind::BareLocal));
        assert!(!classify(b"float x;\nentity e;").contains(&Kind::BareLocal));
    }

    // spec FTE-21.2: a field or function local, with or without `local`.
    #[test]
    fn a_field_or_function_local_is_classified() {
        for source in [
            "void() f = {\nlocal void() pain;\n};",
            "void() f = {\nlocal .float fld;\n};",
            "void() f = {\nvoid() think;\n};",
            "void() f = {\n.float fld;\n};",
        ] {
            assert!(
                classify(source.as_bytes()).contains(&Kind::FieldOrFunctionLocal),
                "{source}"
            );
        }
        assert!(
            !classify(b"void() f = {\nlocal float x;\n};").contains(&Kind::FieldOrFunctionLocal)
        );
    }

    // spec FTE-21.3: a `local` declaration whose line ends at a value with no `;` before the
    // next statement; one continued on the next line is not.
    #[test]
    fn a_missing_semicolon_is_classified() {
        let missing = b"void() f = {\nlocal float c = a + b * 16\nself.colormap = c;\n};";
        assert!(classify(missing).contains(&Kind::MissingSemicolon));
        let continued = b"void() f = {\nlocal float c = a +\nb;\nc = 1;\n};";
        assert!(!classify(continued).contains(&Kind::MissingSemicolon));
    }

    // spec FTE-21.4: an empty nested block; an empty function body and an empty initialiser
    // are not.
    #[test]
    fn an_empty_block_is_classified() {
        assert!(classify(b"void() f = { if (a) { } };").contains(&Kind::EmptyBlock));
        assert!(!classify(b"void() f = { };").contains(&Kind::EmptyBlock));
        assert!(!classify(b"void() f = { float v[] = { }; };").contains(&Kind::EmptyBlock));
    }

    // spec FTE-21.5: a `;` alone after a statement, and a macro whose body ends in `;` used
    // with a `;`, are hanging; the `;` after an initialiser or a body is not.
    #[test]
    fn a_hanging_semicolon_is_classified() {
        assert!(classify(b"void() f = { a = 1;; };").contains(&Kind::HangingSemicolon));
        let macro_use = b"#define STOP if (a) return;\nvoid() f = { STOP; };";
        assert!(classify(macro_use).contains(&Kind::HangingSemicolon));
        let plain_macro = b"#define STOP if (a) return\nvoid() f = { STOP; };";
        assert!(!classify(plain_macro).contains(&Kind::HangingSemicolon));
        let initialiser = b"void() f = { vector v = { 1, 2, 3 }; };";
        assert!(!classify(initialiser).contains(&Kind::HangingSemicolon));
        assert!(!classify(b"void() f = { };").contains(&Kind::HangingSemicolon));
    }

    // spec FTE-21.5: a macro defined in one file and used in another is a hanging `;` in the
    // using file, which the vanilla parser may accept as written.
    #[test]
    fn a_macro_from_another_file_makes_a_hanging_semicolon() {
        let macros = super::semicolon_macros(b"float a;\n#define STOP if (a) return;\n");
        let macros = macros.into_iter().collect();
        let kinds = super::classify_with(b"void() f = { STOP; };\n", &macros);
        assert_eq!(kinds, [Kind::HangingSemicolon]);
        assert!(Kind::HangingSemicolon.after_expansion());
        assert!(!Kind::BareLocal.after_expansion());
        assert_eq!(
            super::semicolon_macros(b"#define STOP x;\n#define GO x\n"),
            ["STOP"]
        );
        assert!(super::semicolon_macros(b"#define\n;").is_empty());
    }

    // spec FTE-8.1: a `case` or `default` label's `:` is not a ternary's.
    #[test]
    fn a_label_colon_is_not_a_ternary() {
        assert!(
            !classify(b"switch (a) {\ncase 0:\nb = 1;\ndefault:\nb = 2;\n}")
                .contains(&Kind::Ternary)
        );
    }

    #[test]
    fn a_codebase_with_every_class_passes_and_bad_arguments_are_errors() {
        let dir = codebase(
            "fteqcc-coverage",
            &[
                // Vanilla as written; its hanging `;` is made by the macro of all.qc.
                ("uses.qc", "void() u = { STOP; };\n"),
                (
                    "all.qc",
                    "const float A = 1;\n\
                 var float b = 0;\n\
                 nosave float c;\n\
                 enum { E };\n\
                 enum Kinds { K };\n\
                 typedef struct { float f; } S;\n\
                 float arr[2];\n\
                 float half = .5;\n\
                 #pragma warning disable F317\n\
                 #define STOP if (b) return;\n\
                 void localsound(entity e, string s) = #0:ex_localsound;\n\
                 void(string s, ...) bprint = #0;\n\
                 void(float a = 0, Kinds k) g;\n\
                 void(entity e) h { };\n\
                 void(float n) f = {\n\
                     for (float i = 0; i < n; i++) {\n\
                         b += i % 2;\n\
                         switch (i) {\n\
                             case 0:\n\
                                 continue;\n\
                             default:\n\
                                 break;\n\
                         }\n\
                         b = i ? 1 : 0;\n\
                     }\n\
                     b = b ^ 1.0f;\n\
                     b = self->health -1;\n\
                     b = -fabs((float) b);\n\
                     local void() pain;\n\
                     local float d = b\n\
                     d = 1;\n\
                     if (d) { }\n\
                     STOP;\n\
                     string t = \"\\t\";\n\
                 };\n",
                ),
            ],
        );
        assert!(main(Path::new("."), &[dir.display().to_string()]).unwrap());
        assert!(main(Path::new("."), &[]).unwrap_err().contains("usage"));
        let empty = codebase("fteqcc-coverage-empty", &[("readme.txt", "x")]);
        assert!(
            main(Path::new("."), &[empty.display().to_string()])
                .unwrap_err()
                .contains("no .qc or .qh")
        );
    }
}
