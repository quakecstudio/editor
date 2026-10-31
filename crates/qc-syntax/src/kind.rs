// SPDX-License-Identifier: GPL-3.0-or-later

//! The kinds of tokens and nodes in the tree.

use qc_lexer::{Punct, TokenKind};

/// What a token or node of the tree is.
///
/// Tokens come first, then nodes. Keyword kinds (`FloatKw` to `DoKw`) are names the parser
/// took as keywords: no name is reserved, so the same word is a [`SyntaxKind::Name`] wherever
/// the parser does not compare it with a keyword (QC-3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SyntaxKind {
    /// Whitespace as `qcc` sees it, including bytes 0x80 to 0xFF (QC-2.1).
    Whitespace,
    /// A `//` comment, without its line break (QC-2.2).
    LineComment,
    /// A `/* */` comment, or an unterminated one to the end of the file (QC-2.3).
    BlockComment,
    /// Bytes the lexer skipped: an unknown byte (QC-7.2), an ignored `$` command line (QC-8.4),
    /// a hanging `$` (QC-8.1), or the text after a NUL byte (QC-1.1).
    Skipped,
    /// A `$frame` line (QC-8.2).
    Frames,
    /// A name (QC-3.1).
    Name,
    /// A float literal (QC-4).
    FloatLit,
    /// A vector literal (QC-5).
    VectorLit,
    /// A string literal (QC-6).
    StringLit,
    /// A frame macro reference, `$word` (QC-8.5).
    FrameRef,
    /// `float` as a type.
    FloatKw,
    /// `vector` as a type.
    VectorKw,
    /// `string` as a type.
    StringKw,
    /// `entity` as a type.
    EntityKw,
    /// `void` as a type.
    VoidKw,
    /// `local` starting a statement.
    LocalKw,
    /// `return` starting a statement.
    ReturnKw,
    /// `if` starting a statement.
    IfKw,
    /// `else` after an `if` statement.
    ElseKw,
    /// `while` starting a statement, or after a `do` body.
    WhileKw,
    /// `do` starting a statement.
    DoKw,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `<=`
    LessEq,
    /// `>=`
    GreaterEq,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `!`
    Not,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `-`
    Minus,
    /// `+`
    Plus,
    /// `=`
    Eq,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `...`
    Ellipsis,
    /// `.`
    Dot,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `#`
    Hash,
    /// `&`
    Amp,
    /// `|`
    Pipe,
    /// `++` (FTEQCC, FTE-5).
    PlusPlus,
    /// `--` (FTEQCC, FTE-5).
    MinusMinus,
    /// `+=` (FTEQCC, FTE-6).
    PlusEq,
    /// `-=` (FTEQCC, FTE-6).
    MinusEq,
    /// `*=` (FTEQCC, FTE-6).
    StarEq,
    /// `/=` (FTEQCC, FTE-6).
    SlashEq,
    /// `%` (FTEQCC, FTE-7).
    Percent,
    /// `?` (FTEQCC, FTE-8).
    Question,
    /// `:` (FTEQCC, FTE-8).
    Colon,
    /// `::` (FTEQCC, FTEF-13.1): a class member's scope.
    ColonColon,
    /// `..` (FTEQCC, FTEF-18.18, FTEF-18.10): a range.
    DotDot,
    /// `->` (FTEQCC, FTE-19.2): member access, as `.`.
    Arrow,
    /// `~` (FTEQCC, FTE-19.4).
    Tilde,
    /// `&=` (FTEQCC, FTE-19.1).
    AndEq,
    /// `|=` (FTEQCC, FTE-19.1).
    OrEq,
    /// `&~=` (FTEQCC, FTE-19.1).
    AndNotEq,
    /// `(+)` (FTEQCC, FTE-19.2): `|=` spelled another way.
    SetBits,
    /// `(-)` (FTEQCC, FTE-19.2): `&~=` spelled another way.
    ClearBits,
    /// `^` (FTEQCC, FTE-19.1).
    Caret,
    /// `^=` (FTEQCC, FTE-19.1).
    CaretEq,
    /// `>>` (FTEQCC, FTE-19.1).
    Shr,
    /// `>>=` (FTEQCC, FTE-19.1).
    ShrEq,
    /// `<<` (FTEQCC, FTE-19.1).
    Shl,
    /// `<<=` (FTEQCC, FTE-19.1).
    ShlEq,
    /// `<=>` (FTEQCC, FTEF-13.24).
    Spaceship,
    /// `*^` (FTEQCC, FTEF-13.24).
    StarCaret,
    /// `><` (FTEQCC, FTEF-13.24).
    Cross,
    /// `@` (FTEQCC, FTEF-13.24).
    At,
    /// `^^` (FTEQCC, FTEF-13.24).
    CaretCaret,

    /// `for` starting a statement (FTEQCC, FTE-2).
    ForKw,
    /// `switch` starting a statement (FTEQCC, FTE-3).
    SwitchKw,
    /// `case` label (FTEQCC, FTE-3).
    CaseKw,
    /// `default` label (FTEQCC, FTE-3).
    DefaultKw,
    /// `break` statement (FTEQCC, FTE-4).
    BreakKw,
    /// `continue` statement (FTEQCC, FTE-4).
    ContinueKw,
    /// `const` qualifier (FTEQCC, FTE-10).
    ConstKw,
    /// `var` qualifier (FTEQCC, FTE-11).
    VarKw,
    /// `nosave` qualifier (FTEQCC, FTE-12).
    NosaveKw,
    /// `typedef` (FTEQCC, FTE-14).
    TypedefKw,
    /// `struct` (FTEQCC, FTE-14).
    StructKw,
    /// `enum` or `enumflags` (FTEQCC, FTE-15, FTEF-8).
    EnumKw,
    /// `class` (FTEQCC, FTEF-9).
    ClassKw,
    /// `union` (FTEQCC, FTEF-6).
    UnionKw,
    /// `accessor` (FTEQCC, FTEF-10).
    AccessorKw,
    /// `goto` (FTEQCC, FTEF-18.23).
    GotoKw,
    /// `until`, by default only `__until` (FTEQCC, FTEF-18.31, FTEF-20.5).
    UntilKw,
    /// `sizeof` (FTEQCC, FTEF-18.12).
    SizeofKw,
    /// `loop` starting a statement (FTEQCC, FTEF-20.5).
    LoopKw,
    /// `thinktime` starting a statement (FTEQCC, FTEF-20.5).
    ThinktimeKw,
    /// `state` starting a statement (FTEQCC, FTEF-20.5).
    StateKw,
    /// `using` starting a statement (FTEQCC, FTEF-20.5).
    UsingKw,
    /// `exit`, only as `__exit`, starting a statement (FTEQCC, FTEF-20.5).
    ExitKw,
    /// `asm` starting a statement (FTEQCC, FTEF-20.5).
    AsmKw,

    /// A whole file.
    SourceFile,
    /// A preprocessor directive line (QC-9.1). The parser treats it as trivia.
    Directive,
    /// A file-level declaration: a type, declarators and `;` (QC-11.1).
    Declaration,
    /// One name of a declaration, with its initialiser if any.
    Declarator,
    /// `void`, `float`, `vector`, `string` or `entity` (QC-10.1).
    SimpleType,
    /// `.T` (QC-10.2).
    FieldType,
    /// `T(...)` (QC-10.3).
    FunctionType,
    /// The parenthesised parameters of a function type.
    ParamList,
    /// One parameter: a type and a name.
    Param,
    /// A function definition's initialiser: an optional state header and a block (QC-13.1).
    FunctionBody,
    /// `[frame, next]` (QC-13.6).
    State,
    /// `#number` (QC-13.3).
    Builtin,
    /// `{ statements }`.
    Block,
    /// `local` and a declaration (QC-11.10).
    LocalDecl,
    /// `return;` or `return e;`.
    ReturnStmt,
    /// `if (e) s` or `if (e) s else s`.
    IfStmt,
    /// `while (e) s`.
    WhileStmt,
    /// `do s while (e);`.
    DoStmt,
    /// `e;`.
    ExprStmt,
    /// A float, vector or string literal, or a frame macro reference.
    Literal,
    /// A name used as a value.
    NameRef,
    /// `(e)`.
    ParenExpr,
    /// `!e` (QC-15.6).
    NotExpr,
    /// `a OP b`, assignment included (QC-15.1).
    BinaryExpr,
    /// `e.f` (QC-15.8).
    FieldExpr,
    /// `f(args)` (QC-15.8).
    CallExpr,
    /// The parenthesised arguments of a call.
    ArgList,
    /// `field: value` or `field = value` among `spawn`'s arguments (FTEQCC, FTEF-17.20).
    NamedArg,

    /// `for (init; cond; step) body` (FTEQCC, FTE-2).
    ForStmt,
    /// One of the three `;`-separated clauses of a `for` header (FTEQCC, FTE-2).
    ForClause,
    /// `switch (e) { ... }` (FTEQCC, FTE-3).
    SwitchStmt,
    /// `case e:` or `default:` (FTEQCC, FTE-3).
    CaseLabel,
    /// `break;` (FTEQCC, FTE-4).
    BreakStmt,
    /// `continue;` (FTEQCC, FTE-4).
    ContinueStmt,
    /// `e++` or `e--` (FTEQCC, FTE-5).
    PostfixExpr,
    /// `cond ? a : b` (FTEQCC, FTE-8).
    TernaryExpr,
    /// `e[i]` (FTEQCC, FTE-13).
    IndexExpr,
    /// `-e`, `+e` or `~e` (FTEQCC, FTE-19.4).
    UnaryExpr,
    /// `(type) e` (FTEQCC, FTE-19.6).
    CastExpr,
    /// `= e` after a parameter's name: its default value (FTEQCC, FTE-18.2).
    DefaultValue,
    /// `;` alone as a statement (FTEQCC, FTE-21.5).
    EmptyStmt,
    /// `const`, `var` or `nosave` before a declaration's type (FTEQCC, FTE-10 to FTE-12).
    Qualifier,
    /// `[N]` or `[]` after a declared name (FTEQCC, FTE-13).
    ArraySuffix,
    /// `{ a, b, ... }` array or struct initialiser (FTEQCC, FTE-13, FTE-14).
    AggregateInit,
    /// `typedef struct { ... } Name;` (FTEQCC, FTE-14).
    TypedefDecl,
    /// `struct { fields }` (FTEQCC, FTE-14).
    StructType,
    /// One field of a struct: a type and a name (FTEQCC, FTE-14).
    StructField,
    /// `enum [class Name] [: type] { ... };` (FTEQCC, FTE-15).
    EnumDecl,
    /// One enumerator `Name` or `Name = value` (FTEQCC, FTE-15).
    Enumerator,
    /// `class Name [: Parent] { members };` (FTEQCC, FTEF-9).
    ClassDecl,
    /// `public:`, `private:`, `protected:`, `virtual:` or `nonvirtual:` in a class (FTEQCC,
    /// FTEF-9.2, FTEF-9.3).
    AccessLabel,
    /// `struct Name { fields };` or `union Name { ... };` at file level (FTEQCC, FTEF-6).
    StructDecl,
    /// `accessor Name : Base [{ members }];` (FTEQCC, FTEF-10).
    AccessorDecl,
    /// One `get` or `set` member of an accessor (FTEQCC, FTEF-10.2).
    AccessorMember,
    /// `T *`, a pointer type (FTEQCC, FTEF-4.1).
    PointerType,
    /// `[x, y, z]`, a vector built from three expressions (FTEQCC, FTEF-16.3).
    VectorExpr,
    /// `name:` or `:name`, a label (FTEQCC, FTEF-18.23).
    LabelStmt,
    /// `goto name;` (FTEQCC, FTEF-18.23).
    GotoStmt,
    /// `sizeof` and a type or an expression (FTEQCC, FTEF-18.12).
    SizeofExpr,
    /// `loop s` (FTEQCC, FTEF-20.5).
    LoopStmt,
    /// `thinktime e : t;` (FTEQCC, FTEF-20.5).
    ThinktimeStmt,
    /// `state [frame, next];` (FTEQCC, FTEF-20.5).
    StateStmt,
    /// `using (names) s` or `using names;` (FTEQCC, FTEF-20.5).
    UsingStmt,
    /// One name of a `using`, and the alias it is given after `=` (FTEQCC, FTEF-20.5).
    UsingAlias,
    /// `__exit;` (FTEQCC, FTEF-20.5).
    ExitStmt,
    /// `asm` and an instruction or a block of them (FTEQCC, FTEF-20.5).
    AsmStmt,
    /// `extern "C"` or `extern "QC"`, or `extern` and any string, and a `{ }` block of
    /// definitions or one definition, read with the flags the string sets (FTEQCC, FTEF-21.17).
    ExternBlock,
    /// `[[ ... ]]` among a declaration's qualifiers: its attributes (FTEQCC, FTEF-21.9).
    AttributeList,
    /// One attribute of an [`SyntaxKind::AttributeList`]: a name and what follows it up to the
    /// next `,` or `]` (FTEQCC, FTEF-21.9).
    Attribute,
    /// Tokens the parser could not place, kept so that no byte is lost.
    Error,
}

impl SyntaxKind {
    /// Whether the kind is trivia: kept in the tree, skipped by the parser.
    #[must_use]
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace
                | Self::LineComment
                | Self::BlockComment
                | Self::Skipped
                | Self::Frames
        )
    }

    /// Whether the kind is a keyword the parser recognised.
    #[must_use]
    pub const fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::FloatKw
                | Self::VectorKw
                | Self::StringKw
                | Self::EntityKw
                | Self::VoidKw
                | Self::LocalKw
                | Self::ReturnKw
                | Self::IfKw
                | Self::ElseKw
                | Self::WhileKw
                | Self::DoKw
                | Self::ForKw
                | Self::SwitchKw
                | Self::CaseKw
                | Self::DefaultKw
                | Self::BreakKw
                | Self::ContinueKw
                | Self::ConstKw
                | Self::VarKw
                | Self::NosaveKw
                | Self::TypedefKw
                | Self::StructKw
                | Self::EnumKw
                | Self::ClassKw
                | Self::UnionKw
                | Self::AccessorKw
                | Self::GotoKw
                | Self::UntilKw
                | Self::SizeofKw
                | Self::LoopKw
                | Self::ThinktimeKw
                | Self::StateKw
                | Self::UsingKw
                | Self::ExitKw
                | Self::AsmKw
        )
    }

    /// The tree kind of a lexer token.
    #[must_use]
    pub const fn from_token_kind(kind: &TokenKind) -> Self {
        match kind {
            TokenKind::Name(_) => Self::Name,
            TokenKind::Float(_) => Self::FloatLit,
            TokenKind::Vector(_) => Self::VectorLit,
            TokenKind::String(_) => Self::StringLit,
            TokenKind::Punct(punct) => Self::from_punct(*punct),
            TokenKind::FrameRef(_) => Self::FrameRef,
            TokenKind::Frames(_) | TokenKind::FrameCommand(_) => Self::Frames,
        }
    }

    /// The kind of a punctuation token.
    #[must_use]
    pub const fn from_punct(punct: Punct) -> Self {
        match punct {
            Punct::AndAnd => Self::AndAnd,
            Punct::OrOr => Self::OrOr,
            Punct::LessEq => Self::LessEq,
            Punct::GreaterEq => Self::GreaterEq,
            Punct::EqEq => Self::EqEq,
            Punct::NotEq => Self::NotEq,
            Punct::Semicolon => Self::Semicolon,
            Punct::Comma => Self::Comma,
            Punct::Not => Self::Not,
            Punct::Star => Self::Star,
            Punct::Slash => Self::Slash,
            Punct::LParen => Self::LParen,
            Punct::RParen => Self::RParen,
            Punct::Minus => Self::Minus,
            Punct::Plus => Self::Plus,
            Punct::Eq => Self::Eq,
            Punct::LBracket => Self::LBracket,
            Punct::RBracket => Self::RBracket,
            Punct::LBrace => Self::LBrace,
            Punct::RBrace => Self::RBrace,
            Punct::Ellipsis => Self::Ellipsis,
            Punct::Dot => Self::Dot,
            Punct::Less => Self::Less,
            Punct::Greater => Self::Greater,
            Punct::Hash => Self::Hash,
            Punct::And => Self::Amp,
            Punct::Or => Self::Pipe,
            Punct::PlusPlus => Self::PlusPlus,
            Punct::MinusMinus => Self::MinusMinus,
            Punct::PlusEq => Self::PlusEq,
            Punct::MinusEq => Self::MinusEq,
            Punct::StarEq => Self::StarEq,
            Punct::SlashEq => Self::SlashEq,
            Punct::Percent => Self::Percent,
            Punct::Question => Self::Question,
            Punct::Colon => Self::Colon,
            Punct::ColonColon => Self::ColonColon,
            Punct::DotDot => Self::DotDot,
            Punct::Arrow => Self::Arrow,
            Punct::Tilde => Self::Tilde,
            Punct::AndEq => Self::AndEq,
            Punct::OrEq => Self::OrEq,
            Punct::AndNotEq => Self::AndNotEq,
            Punct::SetBits => Self::SetBits,
            Punct::ClearBits => Self::ClearBits,
            Punct::Caret => Self::Caret,
            Punct::CaretEq => Self::CaretEq,
            Punct::Shr => Self::Shr,
            Punct::ShrEq => Self::ShrEq,
            Punct::Shl => Self::Shl,
            Punct::ShlEq => Self::ShlEq,
            Punct::Spaceship => Self::Spaceship,
            Punct::StarCaret => Self::StarCaret,
            Punct::Cross => Self::Cross,
            Punct::At => Self::At,
            Punct::CaretCaret => Self::CaretCaret,
        }
    }
}
