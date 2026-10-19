// SPDX-License-Identifier: GPL-3.0-or-later

//! A typed view of the tree.
//!
//! Each type wraps a [`SyntaxNode`] of one kind and names its parts. Every accessor returns
//! an `Option` or an iterator: the tree of a file with errors may lack any part.

use crate::kind::SyntaxKind as K;
use crate::tree::{SyntaxElement, SyntaxNode, SyntaxToken};

/// A typed view of a node.
pub trait AstNode: Sized {
    /// Whether a node of `kind` can be viewed as `Self`.
    fn can_cast(kind: K) -> bool;
    /// The view of `node`, if its kind fits.
    fn cast(node: SyntaxNode) -> Option<Self>;
    /// The node.
    fn syntax(&self) -> &SyntaxNode;
}

macro_rules! ast_node {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        impl AstNode for $name {
            fn can_cast(kind: K) -> bool {
                kind == K::$name
            }

            fn cast(node: SyntaxNode) -> Option<Self> {
                Self::can_cast(node.kind()).then_some(Self(node))
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

macro_rules! ast_enum {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident($node:ident)),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vdoc])* $variant($node),)*
        }

        impl AstNode for $name {
            fn can_cast(kind: K) -> bool {
                $($node::can_cast(kind))||*
            }

            fn cast(node: SyntaxNode) -> Option<Self> {
                $(if $node::can_cast(node.kind()) {
                    return $node::cast(node).map(Self::$variant);
                })*
                None
            }

            fn syntax(&self) -> &SyntaxNode {
                match self {
                    $(Self::$variant(node) => node.syntax(),)*
                }
            }
        }
    };
}

fn child<N: AstNode>(node: &SyntaxNode) -> Option<N> {
    node.children().find_map(N::cast)
}

fn children<N: AstNode>(node: &SyntaxNode) -> impl Iterator<Item = N> {
    node.children().filter_map(N::cast)
}

/// The `N` children before and after the first child token of kind `split`.
fn around<N: AstNode>(node: &SyntaxNode, split: K) -> (Option<N>, Option<N>) {
    let mut before = None;
    let mut after = None;
    let mut seen = false;
    for element in node.children_with_tokens() {
        match element {
            SyntaxElement::Token(token) if token.kind() == split => seen = true,
            SyntaxElement::Token(_) => {}
            SyntaxElement::Node(child) => {
                if let Some(n) = N::cast(child) {
                    if seen {
                        after = after.or(Some(n));
                    } else {
                        before = Some(n);
                    }
                }
            }
        }
    }
    (before, after)
}

ast_node!(
    /// A whole file.
    SourceFile
);
ast_node!(
    /// A preprocessor directive line (QC-9.1).
    Directive
);
ast_node!(
    /// A file-level declaration (QC-11.1).
    Declaration
);
ast_node!(
    /// One name of a declaration and its initialiser.
    Declarator
);
ast_node!(
    /// `void`, `float`, `vector`, `string` or `entity` (QC-10.1).
    SimpleType
);
ast_node!(
    /// `.T` (QC-10.2).
    FieldType
);
ast_node!(
    /// `T(...)` (QC-10.3).
    FunctionType
);
ast_node!(
    /// The parameters of a function type.
    ParamList
);
ast_node!(
    /// One parameter.
    Param
);
ast_node!(
    /// A function's state header and block (QC-13.1).
    FunctionBody
);
ast_node!(
    /// `[frame, next]` (QC-13.6).
    State
);
ast_node!(
    /// `#number` (QC-13.3).
    Builtin
);
ast_node!(
    /// `{ statements }`.
    Block
);
ast_node!(
    /// `local` and a declaration (QC-11.10).
    LocalDecl
);
ast_node!(
    /// `return;` or `return e;`.
    ReturnStmt
);
ast_node!(
    /// `if (e) s [else s]`.
    IfStmt
);
ast_node!(
    /// `while (e) s`.
    WhileStmt
);
ast_node!(
    /// `do s while (e);`.
    DoStmt
);
ast_node!(
    /// `e;`.
    ExprStmt
);
ast_node!(
    /// A literal or frame macro reference.
    Literal
);
ast_node!(
    /// A name used as a value.
    NameRef
);
ast_node!(
    /// `(e)`.
    ParenExpr
);
ast_node!(
    /// `!e`.
    NotExpr
);
ast_node!(
    /// `a OP b`.
    BinaryExpr
);
ast_node!(
    /// `e.f`.
    FieldExpr
);
ast_node!(
    /// `f(args)`.
    CallExpr
);
ast_node!(
    /// The arguments of a call.
    ArgList
);

ast_node!(
    /// `for (init; cond; step) body` (FTE-2).
    ForStmt
);
ast_node!(
    /// `switch (e) { ... }` (FTE-3).
    SwitchStmt
);
ast_node!(
    /// `case e:` or `default:` (FTE-3).
    CaseLabel
);
ast_node!(
    /// `break;` (FTE-4).
    BreakStmt
);
ast_node!(
    /// `continue;` (FTE-4).
    ContinueStmt
);
ast_node!(
    /// `e++` or `e--` (FTE-5).
    PostfixExpr
);
ast_node!(
    /// `cond ? a : b` (FTE-8).
    TernaryExpr
);
ast_node!(
    /// `e[i]` (FTE-13).
    IndexExpr
);
ast_node!(
    /// `-e`, `+e` or `~e` (FTE-19.4).
    UnaryExpr
);
ast_node!(
    /// `;` alone (FTE-21.5).
    EmptyStmt
);
ast_node!(
    /// `(type) e` (FTE-19.6).
    CastExpr
);
ast_node!(
    /// A parameter's default value, `= e` (FTE-18.2).
    DefaultValue
);
ast_node!(
    /// `{ a, b, ... }` (FTE-13, FTE-14).
    AggregateInit
);
ast_node!(
    /// `typedef struct { ... } Name;` (FTE-14).
    TypedefDecl
);
ast_node!(
    /// `enum ... { ... };` (FTE-15).
    EnumDecl
);

ast_enum!(
    /// A type (QC-10).
    Type {
        /// `void`, `float`, `vector`, `string` or `entity`.
        Simple(SimpleType),
        /// `.T`.
        Field(FieldType),
        /// `T(...)`.
        Function(FunctionType),
    }
);

ast_enum!(
    /// A statement (QC-14).
    Stmt {
        /// `{ ... }`.
        Block(Block),
        /// `local ...;`.
        Local(LocalDecl),
        /// `return ...;`.
        Return(ReturnStmt),
        /// `if ...`.
        If(IfStmt),
        /// `while ...`.
        While(WhileStmt),
        /// `do ... while (...);`.
        Do(DoStmt),
        /// `for (...) ...` (FTE-2).
        For(ForStmt),
        /// `switch (...) { ... }` (FTE-3).
        Switch(SwitchStmt),
        /// `break;` (FTE-4).
        Break(BreakStmt),
        /// `continue;` (FTE-4).
        Continue(ContinueStmt),
        /// `;` alone (FTE-21.5).
        Empty(EmptyStmt),
        /// `e;`.
        Expr(ExprStmt),
    }
);

ast_enum!(
    /// An expression (QC-15).
    Expr {
        /// A literal.
        Literal(Literal),
        /// A name.
        Name(NameRef),
        /// `(e)`.
        Paren(ParenExpr),
        /// `!e`.
        Not(NotExpr),
        /// `a OP b`.
        Binary(BinaryExpr),
        /// `e.f`.
        Field(FieldExpr),
        /// `f(args)`.
        Call(CallExpr),
        /// `e++` or `e--` (FTE-5).
        Postfix(PostfixExpr),
        /// `cond ? a : b` (FTE-8).
        Ternary(TernaryExpr),
        /// `e[i]` (FTE-13).
        Index(IndexExpr),
        /// `-e`, `+e` or `~e` (FTE-19.4).
        Unary(UnaryExpr),
        /// `(type) e` (FTE-19.6).
        Cast(CastExpr),
    }
);

ast_enum!(
    /// What follows `=` in a declarator.
    Initializer {
        /// A function body (QC-13.1).
        Body(FunctionBody),
        /// A builtin number (QC-13.3).
        Builtin(Builtin),
        /// An array or struct initialiser `{ ... }` (FTE-13, FTE-14).
        Aggregate(AggregateInit),
        /// A constant (QC-11.3).
        Value(Expr),
    }
);

/// What a declaration's members are, shared by [`Declaration`] and [`LocalDecl`].
fn decl_type(node: &SyntaxNode) -> Option<Type> {
    child(node)
}

impl SourceFile {
    /// The file-level declarations, in order.
    pub fn declarations(&self) -> impl Iterator<Item = Declaration> {
        children(&self.0)
    }

    /// The directive lines between declarations.
    pub fn directives(&self) -> impl Iterator<Item = Directive> {
        children(&self.0)
    }
}

impl Declaration {
    /// The declared type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        decl_type(&self.0)
    }

    /// The declared names, in order.
    pub fn declarators(&self) -> impl Iterator<Item = Declarator> {
        children(&self.0)
    }
}

impl LocalDecl {
    /// The `local` keyword; `None` when it is missing (an error).
    #[must_use]
    pub fn local_token(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::LocalKw)
    }

    /// The declared type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        decl_type(&self.0)
    }

    /// The declared names, in order.
    pub fn declarators(&self) -> impl Iterator<Item = Declarator> {
        children(&self.0)
    }
}

impl Declarator {
    /// The declared name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }

    /// What follows `=`, or the body of a function declarator written without `=`
    /// (FTE-17.2).
    #[must_use]
    pub fn initializer(&self) -> Option<Initializer> {
        child(&self.0)
    }

    /// The parameters of an FTEQCC C-style function declarator `rettype name(params)`
    /// (FTE-17.1); `None` for every other declarator.
    #[must_use]
    pub fn params(&self) -> Option<ParamList> {
        child(&self.0)
    }
}

/// The five simple types (QC-10.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SimpleTypeKind {
    /// `void`
    Void,
    /// `float`
    Float,
    /// `vector`
    Vector,
    /// `string`
    String,
    /// `entity`
    Entity,
}

impl SimpleType {
    /// The keyword.
    #[must_use]
    pub fn keyword(&self) -> Option<SyntaxToken> {
        self.0.child_tokens().find(|t| t.kind().is_keyword())
    }

    /// Which type it is.
    #[must_use]
    pub fn kind(&self) -> Option<SimpleTypeKind> {
        match self.keyword()?.kind() {
            K::VoidKw => Some(SimpleTypeKind::Void),
            K::FloatKw => Some(SimpleTypeKind::Float),
            K::VectorKw => Some(SimpleTypeKind::Vector),
            K::StringKw => Some(SimpleTypeKind::String),
            K::EntityKw => Some(SimpleTypeKind::Entity),
            _ => None,
        }
    }
}

impl FieldType {
    /// The type of the field's value.
    #[must_use]
    pub fn inner(&self) -> Option<Type> {
        child(&self.0)
    }
}

impl FunctionType {
    /// The return type.
    #[must_use]
    pub fn return_type(&self) -> Option<SimpleType> {
        child(&self.0)
    }

    /// The parameters.
    #[must_use]
    pub fn params(&self) -> Option<ParamList> {
        child(&self.0)
    }
}

impl ParamList {
    /// The parameters, in order; none for `(...)`.
    pub fn params(&self) -> impl Iterator<Item = Param> {
        children(&self.0)
    }

    /// Whether it is `(...)` (QC-10.4).
    #[must_use]
    pub fn is_variadic(&self) -> bool {
        self.0.token_of(K::Ellipsis).is_some()
    }
}

impl Param {
    /// The parameter's type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// The parameter's name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }
}

impl FunctionBody {
    /// The `[frame, next]` header, if any.
    #[must_use]
    pub fn state(&self) -> Option<State> {
        child(&self.0)
    }

    /// The block.
    #[must_use]
    pub fn block(&self) -> Option<Block> {
        child(&self.0)
    }
}

impl State {
    /// The frame: a float literal or frame macro reference.
    #[must_use]
    pub fn frame(&self) -> Option<Literal> {
        child(&self.0)
    }

    /// The next think function's name.
    #[must_use]
    pub fn next(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }
}

impl Builtin {
    /// The builtin number: a float literal or frame macro reference.
    #[must_use]
    pub fn number(&self) -> Option<SyntaxToken> {
        self.0
            .child_tokens()
            .find(|t| matches!(t.kind(), K::FloatLit | K::FrameRef))
    }
}

impl Block {
    /// The statements, in order.
    pub fn statements(&self) -> impl Iterator<Item = Stmt> {
        children(&self.0)
    }
}

impl ReturnStmt {
    /// The returned value, if any.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

impl IfStmt {
    /// The condition.
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The statement run when the condition holds.
    #[must_use]
    pub fn then_branch(&self) -> Option<Stmt> {
        around(&self.0, K::ElseKw).0
    }

    /// The statement after `else`, if any.
    #[must_use]
    pub fn else_branch(&self) -> Option<Stmt> {
        around(&self.0, K::ElseKw).1
    }
}

impl WhileStmt {
    /// The condition.
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The loop body.
    #[must_use]
    pub fn body(&self) -> Option<Stmt> {
        child(&self.0)
    }
}

impl DoStmt {
    /// The loop body.
    #[must_use]
    pub fn body(&self) -> Option<Stmt> {
        child(&self.0)
    }

    /// The condition.
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        child(&self.0)
    }
}

impl ExprStmt {
    /// The expression.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

/// What a literal is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LiteralKind {
    /// A float (QC-4).
    Float,
    /// A vector (QC-5).
    Vector,
    /// A string (QC-6).
    String,
    /// A frame macro reference, a float once resolved (QC-8.5).
    FrameRef,
}

impl Literal {
    /// The literal's token.
    #[must_use]
    pub fn token(&self) -> Option<SyntaxToken> {
        self.0.child_tokens().find(|t| !t.kind().is_trivia())
    }

    /// What it is.
    #[must_use]
    pub fn kind(&self) -> Option<LiteralKind> {
        match self.token()?.kind() {
            K::FloatLit => Some(LiteralKind::Float),
            K::VectorLit => Some(LiteralKind::Vector),
            K::StringLit => Some(LiteralKind::String),
            K::FrameRef => Some(LiteralKind::FrameRef),
            _ => None,
        }
    }
}

impl NameRef {
    /// The name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }
}

impl ParenExpr {
    /// The expression inside.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

impl NotExpr {
    /// The operand.
    #[must_use]
    pub fn operand(&self) -> Option<Expr> {
        child(&self.0)
    }
}

/// A binary operator (QC-15.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `&`
    BitAnd,
    /// `|`
    BitOr,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `=`
    Assign,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `%` (FTEQCC, FTE-7).
    Rem,
    /// `^` (FTEQCC, FTE-19.1).
    BitXor,
    /// `<<` (FTEQCC, FTE-19.1).
    Shl,
    /// `>>` (FTEQCC, FTE-19.1).
    Shr,
    /// `+=` (FTEQCC, FTE-6).
    AddAssign,
    /// `-=` (FTEQCC, FTE-6).
    SubAssign,
    /// `*=` (FTEQCC, FTE-6).
    MulAssign,
    /// `/=` (FTEQCC, FTE-6).
    DivAssign,
    /// `&=` (FTEQCC, FTE-19.1).
    BitAndAssign,
    /// `|=` or `(+)` (FTEQCC, FTE-19.1, FTE-19.2).
    BitOrAssign,
    /// `&~=` or `(-)` (FTEQCC, FTE-19.1, FTE-19.2): clear the right side's bits.
    BitClearAssign,
    /// `^=` (FTEQCC, FTE-19.1).
    BitXorAssign,
    /// `<<=` (FTEQCC, FTE-19.1).
    ShlAssign,
    /// `>>=` (FTEQCC, FTE-19.1).
    ShrAssign,
}

impl BinaryOp {
    const fn from_kind(kind: K) -> Option<Self> {
        Some(match kind {
            K::Star => Self::Mul,
            K::Slash => Self::Div,
            K::Amp => Self::BitAnd,
            K::Pipe => Self::BitOr,
            K::Plus => Self::Add,
            K::Minus => Self::Sub,
            K::EqEq => Self::Eq,
            K::NotEq => Self::Ne,
            K::LessEq => Self::Le,
            K::GreaterEq => Self::Ge,
            K::Less => Self::Lt,
            K::Greater => Self::Gt,
            K::Eq => Self::Assign,
            K::AndAnd => Self::And,
            K::OrOr => Self::Or,
            K::Percent => Self::Rem,
            K::Caret => Self::BitXor,
            K::Shl => Self::Shl,
            K::Shr => Self::Shr,
            K::PlusEq => Self::AddAssign,
            K::MinusEq => Self::SubAssign,
            K::StarEq => Self::MulAssign,
            K::SlashEq => Self::DivAssign,
            K::AndEq => Self::BitAndAssign,
            K::OrEq | K::SetBits => Self::BitOrAssign,
            K::AndNotEq | K::ClearBits => Self::BitClearAssign,
            K::CaretEq => Self::BitXorAssign,
            K::ShlEq => Self::ShlAssign,
            K::ShrEq => Self::ShrAssign,
            _ => return None,
        })
    }
}

impl BinaryExpr {
    /// The operator's token; `None` for FTEQCC's `a -1`, whose operator is in the negative
    /// number (FTE-19.5).
    #[must_use]
    pub fn op_token(&self) -> Option<SyntaxToken> {
        self.0
            .child_tokens()
            .find(|t| BinaryOp::from_kind(t.kind()).is_some())
    }

    /// The operator. FTEQCC reads `a -1` as `a + -1` (FTE-19.5), so a binary expression with
    /// no operator token is [`BinaryOp::Add`].
    #[must_use]
    pub fn op(&self) -> Option<BinaryOp> {
        match self.op_token() {
            Some(token) => BinaryOp::from_kind(token.kind()),
            None => self.rhs().map(|_| BinaryOp::Add),
        }
    }

    /// The left operand.
    #[must_use]
    pub fn lhs(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The right operand.
    #[must_use]
    pub fn rhs(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }
}

impl FieldExpr {
    /// The entity expression before `.` or the FTEQCC `->`.
    #[must_use]
    pub fn base(&self) -> Option<Expr> {
        let split = if self.0.token_of(K::Arrow).is_some() {
            K::Arrow
        } else {
            K::Dot
        };
        around(&self.0, split).0
    }

    /// The field expression after `.` or the FTEQCC `->`.
    #[must_use]
    pub fn field(&self) -> Option<Expr> {
        let split = if self.0.token_of(K::Arrow).is_some() {
            K::Arrow
        } else {
            K::Dot
        };
        around(&self.0, split).1
    }
}

impl CallExpr {
    /// The function called.
    #[must_use]
    pub fn callee(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The arguments.
    #[must_use]
    pub fn args(&self) -> Option<ArgList> {
        child(&self.0)
    }
}

impl ArgList {
    /// The arguments, in order.
    pub fn args(&self) -> impl Iterator<Item = Expr> {
        children(&self.0)
    }
}

impl ForStmt {
    /// The three header clauses in order: init, cond and step (FTE-2.1). A clause that was
    /// empty has no node, so fewer than three may be returned.
    pub fn clauses(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|n| n.kind() == K::ForClause)
    }

    /// The loop body: the statement after the header's `)`.
    #[must_use]
    pub fn body(&self) -> Option<Stmt> {
        self.0.children().find_map(Stmt::cast)
    }
}

impl SwitchStmt {
    /// The switched expression (FTE-3.1).
    #[must_use]
    pub fn scrutinee(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The switch body block, holding the labels and statements.
    #[must_use]
    pub fn block(&self) -> Option<Block> {
        child(&self.0)
    }
}

impl CaseLabel {
    /// The `case` value, or `None` for `default:` (FTE-3.1).
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// Whether this is the `default:` label.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.0.token_of(K::DefaultKw).is_some()
    }
}

impl PostfixExpr {
    /// The operand (FTE-5.1).
    #[must_use]
    pub fn operand(&self) -> Option<Expr> {
        child(&self.0)
    }
}

impl TernaryExpr {
    /// The condition (FTE-8.1).
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        around(&self.0, K::Question).0
    }

    /// The arm chosen when the condition holds, and the arm chosen otherwise (FTE-8.1).
    #[must_use]
    pub fn arms(&self) -> (Option<Expr>, Option<Expr>) {
        let after = around::<Expr>(&self.0, K::Question).1;
        let otherwise = around::<Expr>(&self.0, K::Colon).1;
        (after, otherwise)
    }
}

impl IndexExpr {
    /// The base expression being subscripted (FTE-13.2).
    #[must_use]
    pub fn base(&self) -> Option<Expr> {
        around(&self.0, K::LBracket).0
    }

    /// The index expression inside `[ ]`.
    #[must_use]
    pub fn index(&self) -> Option<Expr> {
        around(&self.0, K::LBracket).1
    }
}

/// A prefix operator of FTEQCC (FTE-19.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `-`: negation.
    Neg,
    /// `+`: the operand unchanged.
    Plus,
    /// `~`: bitwise not.
    BitNot,
}

impl UnaryExpr {
    /// The operator.
    #[must_use]
    pub fn op(&self) -> Option<UnaryOp> {
        self.0.child_tokens().find_map(|t| match t.kind() {
            K::Minus => Some(UnaryOp::Neg),
            K::Plus => Some(UnaryOp::Plus),
            K::Tilde => Some(UnaryOp::BitNot),
            _ => None,
        })
    }

    /// The operand.
    #[must_use]
    pub fn operand(&self) -> Option<Expr> {
        child(&self.0)
    }
}

impl CastExpr {
    /// The type cast to.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// The value cast.
    #[must_use]
    pub fn operand(&self) -> Option<Expr> {
        child(&self.0)
    }
}

impl Param {
    /// The parameter's default value (FTE-18.2), if it has one.
    #[must_use]
    pub fn default_value(&self) -> Option<Expr> {
        child::<DefaultValue>(&self.0).and_then(|d| child(&d.0))
    }
}

impl AggregateInit {
    /// The elements of the initialiser, in order (FTE-13.1, FTE-14.3). A nested aggregate is
    /// not an [`Expr`], so only the expression elements are returned here.
    pub fn elements(&self) -> impl Iterator<Item = Expr> {
        children(&self.0)
    }
}

impl TypedefDecl {
    /// The type's name (FTE-14.1): the last name token in the declaration.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.child_tokens().filter(|t| t.kind() == K::Name).last()
    }
}

impl EnumDecl {
    /// The enum's type name, `enum Name { ... }` or `enum class Name ...` (FTE-15.4); `None`
    /// for an anonymous enum.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .child_tokens()
            .take_while(|t| t.kind() != K::LBrace && t.kind() != K::Colon)
            .filter(|t| t.kind() == K::Name)
            .find(|t| t.text_lossless() != "class")
    }

    /// The enumerators, in order (FTE-15.1).
    pub fn enumerators(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|n| n.kind() == K::Enumerator)
    }
}
