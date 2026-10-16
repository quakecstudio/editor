// SPDX-License-Identifier: GPL-3.0-or-later

//! The kinds of tokens and nodes in the tree.

use qc_lexer::Punct;

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
        )
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
        }
    }
}
