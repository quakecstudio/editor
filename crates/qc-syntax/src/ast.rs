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
    }
);

ast_enum!(
    /// What follows `=` in a declarator.
    Initializer {
        /// A function body (QC-13.1).
        Body(FunctionBody),
        /// A builtin number (QC-13.3).
        Builtin(Builtin),
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

    /// What follows `=`.
    #[must_use]
    pub fn initializer(&self) -> Option<Initializer> {
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
}

impl BinaryOp {
    fn from_kind(kind: K) -> Option<Self> {
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
            _ => return None,
        })
    }
}

impl BinaryExpr {
    /// The operator's token.
    #[must_use]
    pub fn op_token(&self) -> Option<SyntaxToken> {
        self.0
            .child_tokens()
            .find(|t| BinaryOp::from_kind(t.kind()).is_some())
    }

    /// The operator.
    #[must_use]
    pub fn op(&self) -> Option<BinaryOp> {
        BinaryOp::from_kind(self.op_token()?.kind())
    }

    /// The left operand.
    #[must_use]
    pub fn lhs(&self) -> Option<Expr> {
        let kind = self.op_token()?.kind();
        around(&self.0, kind).0
    }

    /// The right operand.
    #[must_use]
    pub fn rhs(&self) -> Option<Expr> {
        let kind = self.op_token()?.kind();
        around(&self.0, kind).1
    }
}

impl FieldExpr {
    /// The entity expression before `.`.
    #[must_use]
    pub fn base(&self) -> Option<Expr> {
        around(&self.0, K::Dot).0
    }

    /// The field expression after `.`.
    #[must_use]
    pub fn field(&self) -> Option<Expr> {
        around(&self.0, K::Dot).1
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
