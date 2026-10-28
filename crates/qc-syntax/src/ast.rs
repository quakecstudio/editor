// SPDX-License-Identifier: GPL-3.0-or-later

//! A typed view of the tree.
//!
//! Each type wraps a [`SyntaxNode`] of one kind and names its parts. Every accessor returns
//! an `Option` or an iterator: the tree of a file with errors may lack any part.
//!
//! The FTEQCC limits quoted here, such as the length of a builtin's engine name (FTE-23.6), are
//! read from fte-team/fteqw for facts only: no function of it is ported into this file, so it
//! carries no `Ported from` header (ADR-0017).

use crate::kind::SyntaxKind as K;
use crate::tree::{SyntaxElement, SyntaxNode, SyntaxToken};

/// The bytes of an engine name FTEQCC keeps: `QC_strlcpy` into `char fname[256]` in
/// `QCC_PR_ParseInitializerType_Internal` (`qcc_pr_comp.c`,
/// fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e) copies at most 255 (FTE-23.6).
const FTEQCC_MAX_ENGINE_NAME: usize = 255;

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
    /// `field: value` or `field = value` among `spawn`'s arguments (FTEF-17.20).
    NamedArg
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
ast_node!(
    /// `class Name [: Parent] { members };` (FTEF-9).
    ClassDecl
);
ast_node!(
    /// `struct Name { fields };` or `union Name { ... };` (FTEF-6).
    StructDecl
);
ast_node!(
    /// `accessor Name : Base { members };` (FTEF-10).
    AccessorDecl
);
ast_node!(
    /// A `get` or `set` member of an accessor (FTEF-10.2).
    AccessorMember
);
ast_node!(
    /// `T *` (FTEF-4.1).
    PointerType
);
ast_node!(
    /// `[x, y, z]` (FTEF-16.3).
    VectorExpr
);
ast_node!(
    /// `name:` or `:name` (FTEF-18.23).
    LabelStmt
);
ast_node!(
    /// `goto name;` (FTEF-18.23).
    GotoStmt
);
ast_node!(
    /// `sizeof` and a type or an expression (FTEF-18.12).
    SizeofExpr
);
ast_node!(
    /// `loop s` (FTEF-20.5).
    LoopStmt
);
ast_node!(
    /// `thinktime e : t;` (FTEF-20.5).
    ThinktimeStmt
);
ast_node!(
    /// `state [frame, next];` (FTEF-20.5).
    StateStmt
);
ast_node!(
    /// `using (names) s` or `using names;` (FTEF-20.5).
    UsingStmt
);
ast_node!(
    /// One name of a `using` and its alias (FTEF-20.5).
    UsingAlias
);
ast_node!(
    /// `__exit;` (FTEF-20.5).
    ExitStmt
);
ast_node!(
    /// `asm` and an instruction or a block of them (FTEF-20.5).
    AsmStmt
);
ast_node!(
    /// `extern` and a string, and a `{ }` block of definitions or one definition (FTEF-21.17).
    ExternBlock
);
ast_node!(
    /// `[N]` or `[]` after a declared name or a parameter's (FTE-13.1, FTEF-15.3).
    ArraySuffix
);

impl ArraySuffix {
    /// Whether it is `[]`, with no size.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.children().next().is_none()
    }
}
ast_node!(
    /// `[[ ... ]]` among a declaration's qualifiers (FTEF-21.9).
    AttributeList
);
ast_node!(
    /// One attribute of an [`AttributeList`] (FTEF-21.9).
    Attribute
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
        /// `T *` (FTEF-4.1).
        Pointer(PointerType),
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
        /// `name:` or `:name` (FTEF-18.23).
        Label(LabelStmt),
        /// `goto name;` (FTEF-18.23).
        Goto(GotoStmt),
        /// `loop s` (FTEF-20.5).
        Loop(LoopStmt),
        /// `thinktime e : t;` (FTEF-20.5).
        Thinktime(ThinktimeStmt),
        /// `state [frame, next];` (FTEF-20.5).
        State(StateStmt),
        /// `using (names) s` or `using names;` (FTEF-20.5).
        Using(UsingStmt),
        /// `__exit;` (FTEF-20.5).
        Exit(ExitStmt),
        /// `asm` and its instructions (FTEF-20.5).
        Asm(AsmStmt),
        /// `extern "C" { ... }` and its local definitions (FTEF-21.17).
        Extern(ExternBlock),
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
        /// `[x, y, z]` (FTEF-16.3).
        Vector(VectorExpr),
        /// `sizeof` and a type or an expression (FTEF-18.12).
        Sizeof(SizeofExpr),
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

/// Pushes the children of `node` onto `out`, those of each `extern` block in its place.
fn flatten_externs(node: &SyntaxNode, out: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        if child.kind() == K::ExternBlock {
            flatten_externs(&child, out);
        } else {
            out.push(child);
        }
    }
}

impl ExternBlock {
    /// What the block holds, in order: each node a definition is, a nested block's in its place
    /// (FTEF-21.17).
    #[must_use]
    pub fn items(&self) -> Vec<SyntaxNode> {
        let mut items = Vec::new();
        flatten_externs(&self.0, &mut items);
        items
    }
}

impl AttributeList {
    /// The attributes, in order.
    pub fn attributes(&self) -> impl Iterator<Item = Attribute> {
        children(&self.0)
    }
}

impl Attribute {
    /// The attribute's name: its first token.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.child_tokens().find(|t| !t.kind().is_trivia())
    }

    /// The tokens after the name, up to the attribute's end, trivia aside: an argument list
    /// `( ... )` for `alias` and `__deprecated`.
    pub fn arguments(&self) -> impl Iterator<Item = SyntaxToken> {
        self.0
            .child_tokens()
            .filter(|t| !t.kind().is_trivia())
            .skip(1)
    }
}

/// What a declaration's members are, shared by [`Declaration`] and [`LocalDecl`].
fn decl_type(node: &SyntaxNode) -> Option<Type> {
    child(node)
}

impl SourceFile {
    /// The file-level declarations, in order, those of an `extern` block among them in its
    /// place (FTEF-21.17).
    pub fn declarations(&self) -> impl Iterator<Item = Declaration> {
        self.items().into_iter().filter_map(Declaration::cast)
    }

    /// The file-level items, in order: each node a definition is, the items of an `extern`
    /// block in its place rather than the block (FTEF-21.17).
    #[must_use]
    pub fn items(&self) -> Vec<SyntaxNode> {
        let mut items = Vec::new();
        flatten_externs(&self.0, &mut items);
        items
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

/// The name of a possibly scoped name `Name` or `Scope::Name` among `node`'s tokens: the name
/// after the last `::` (FTEF-9.4, FTEF-16.4). A declarator or a name reference holds one name,
/// or names joined by `::`, so it is the last name.
fn scoped_name(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.child_tokens().filter(|t| t.kind() == K::Name).last()
}

/// The scope of a scoped name `Scope::Name`: the name before the first `::`.
fn scope_of(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut last_name = None;
    for token in node.child_tokens() {
        match token.kind() {
            K::ColonColon => return last_name,
            K::Name => last_name = Some(token),
            _ => {}
        }
    }
    None
}

impl Declarator {
    /// The declared name: for `Class::member` (FTEF-9.4), the member's.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        scoped_name(&self.0)
    }

    /// The class of a `Class::member` declarator (FTEF-9.4); `None` for a plain name.
    #[must_use]
    pub fn scope(&self) -> Option<SyntaxToken> {
        scope_of(&self.0)
    }

    /// How many `*` come before the name and any `(`: `T *p` declares a pointer (FTEF-4.1).
    #[must_use]
    pub fn pointer_depth(&self) -> usize {
        head_stars(&self.0)
    }

    /// The parameter lists before the name, each a function returning the type so far
    /// (FTEF-21.7).
    #[must_use]
    pub fn prefix_params(&self) -> Vec<ParamList> {
        prefix_lists(&self.0, self.name().as_ref())
    }

    /// For C's function pointer `(*name)(params)`, how many `*` the parentheses hold before the
    /// name (FTEF-21.7); `None` for any other declarator.
    #[must_use]
    pub fn pointer_form(&self) -> Option<usize> {
        pointer_form(&self.0)
    }

    /// The array suffixes after the name (FTE-13.1, FTEF-5).
    pub fn array_suffixes(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|n| n.kind() == K::ArraySuffix)
    }

    /// What follows `=`, or the body of a function declarator written without `=`
    /// (FTE-17.2).
    #[must_use]
    pub fn initializer(&self) -> Option<Initializer> {
        child(&self.0)
    }

    /// The parameters of an FTEQCC C-style function declarator `rettype name(params)`
    /// (FTE-17.1), or of C's function pointer `(*name)(params)` (FTEF-21.7): the list after the
    /// name; `None` for every other declarator.
    #[must_use]
    pub fn params(&self) -> Option<ParamList> {
        suffix_list(&self.0, self.name().as_ref())
    }
}

/// How many `*` tokens of `node` come before its first name and `(` (FTEF-4.1, FTEF-21.7).
fn head_stars(node: &SyntaxNode) -> usize {
    node.child_tokens()
        .take_while(|t| !matches!(t.kind(), K::Name | K::LParen))
        .filter(|t| t.kind() == K::Star)
        .count()
}

/// The `ParamList` children of `node` before the token `name` (FTEF-21.7).
fn prefix_lists(node: &SyntaxNode, name: Option<&SyntaxToken>) -> Vec<ParamList> {
    let end = name.map_or(0, |t| t.span().start);
    node.children()
        .filter(|n| n.span().end <= end)
        .filter_map(ParamList::cast)
        .collect()
}

/// The first `ParamList` child of `node` after the token `name`.
fn suffix_list(node: &SyntaxNode, name: Option<&SyntaxToken>) -> Option<ParamList> {
    let start = name?.span().end;
    node.children()
        .filter(|n| n.span().start >= start)
        .find_map(ParamList::cast)
}

/// For C's function pointer `(*name)`, the `*` tokens between the `(` and the name; `None` when
/// no `(` token of `node` comes before its first name (FTEF-21.7).
fn pointer_form(node: &SyntaxNode) -> Option<usize> {
    let mut tokens = node
        .child_tokens()
        .skip_while(|t| t.kind() != K::LParen && t.kind() != K::Name);
    if tokens.next()?.kind() != K::LParen {
        return None;
    }
    Some(
        tokens
            .take_while(|t| t.kind() != K::Name)
            .filter(|t| t.kind() == K::Star)
            .count(),
    )
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

    /// The type's words as written, joined by a space: a vanilla keyword, the C type words
    /// (FTEF-2), `struct tag` (FTEF-6.1) or a type name a program defines (FTEF-7.2).
    #[must_use]
    pub fn text(&self) -> String {
        let words: Vec<String> = self
            .0
            .child_tokens()
            .filter(|t| !t.kind().is_trivia())
            .map(|t| t.text_lossless())
            .collect();
        words.join(" ")
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
    /// The return type: a simple type in vanilla (QC-10.3), any type in FTEQCC (FTEF-15.2).
    #[must_use]
    pub fn return_type(&self) -> Option<Type> {
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

    /// The array suffixes after its name (FTEF-15.3), in order.
    #[must_use]
    pub fn array_suffixes(&self) -> Vec<ArraySuffix> {
        children(&self.0).collect()
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

    /// The next think function's name, when it is written as a name (QC-13.6). An FTEQCC
    /// frame cycle (FTEF-18.28) has none, and a next function written as another expression
    /// (FTEF-18.3) is among [`State::exprs`].
    #[must_use]
    pub fn next(&self) -> Option<SyntaxToken> {
        if self.is_cycle() {
            return None;
        }
        self.0.token_of(K::Name)
    }

    /// Whether this is an FTEQCC frame cycle, `[++ first .. last]` or `[-- first .. last]`
    /// (FTEF-18.28).
    #[must_use]
    pub fn is_cycle(&self) -> bool {
        self.0.token_of(K::DotDot).is_some()
    }

    /// The expressions of the header: the frame and, unless it is a name, the next function
    /// (FTEF-18.3); or a cycle's first and last frames (FTEF-18.28).
    pub fn exprs(&self) -> impl Iterator<Item = Expr> {
        children(&self.0)
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

    /// The engine name an FTEQCC builtin initialiser binds: the name after `#number:` or `:`
    /// (FTE-17.4, FTE-17.5), or the value of the strings after `#` or `:` (FTE-17.5), joined
    /// as FTEQCC's lexer joins adjacent strings and their escapes read as it reads them
    /// (FTE-23.5), up to the first NUL byte; an empty string names nothing, so the name after it
    /// is the one. The name is kept to the 255 bytes of FTEQCC's `fname` (FTE-23.6), each byte
    /// one `char` (Latin-1), as FTEQCC compares names byte for byte. The strings are read with
    /// `flags`, the compiler flags they were lexed with (FTEF-12.12, FTEF-19.23).
    #[must_use]
    pub fn engine_name(&self, flags: qc_lexer::LexFlags) -> Option<String> {
        let mut joined = Vec::new();
        let mut name = None;
        for token in self.0.child_tokens() {
            match token.kind() {
                // FTEF-13.7: a part may have a prefix, an `L` among them after another part.
                K::StringLit => {
                    // FTEF-14.56: a NUL byte of the file is a space in the loader's text.
                    let text: Vec<u8> = token
                        .text()
                        .iter()
                        .map(|&byte| if byte == 0 { b' ' } else { byte })
                        .collect();
                    if let Some(bytes) = qc_lexer::fteqcc_string_bytes(&text, flags) {
                        joined.extend_from_slice(&bytes);
                    }
                }
                K::Name => name = Some(token),
                _ => {}
            }
        }
        // FTE-23.6: `QC_strlcpy` copies a C string, which ends at its first NUL.
        if let Some(end) = joined.iter().position(|&b| b == 0) {
            joined.truncate(end);
        }
        let bytes = if joined.is_empty() {
            name?.text().to_vec()
        } else {
            joined
        };
        let kept = bytes.get(..FTEQCC_MAX_ENGINE_NAME).unwrap_or(&bytes);
        Some(kept.iter().copied().map(char::from).collect())
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

    /// Whether it is FTEQCC's `return = e;`, which sets the value without returning
    /// (FTEF-18.7).
    #[must_use]
    pub fn sets(&self) -> bool {
        self.0.token_of(K::Eq).is_some()
    }
}

/// How an `if` negates its condition (FTEF-16.5, FTEF-18.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Negation {
    /// `if not (e)`.
    Not,
    /// `if !(e)`.
    Bang,
}

impl IfStmt {
    /// How it negates its condition, if it does.
    #[must_use]
    pub fn negation(&self) -> Option<Negation> {
        if self.0.token_of(K::Not).is_some() {
            Some(Negation::Bang)
        } else {
            self.0.token_of(K::Name).map(|_| Negation::Not)
        }
    }

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
    /// Whether it is `until`, which loops while the condition is false (FTEF-18.31).
    #[must_use]
    pub fn until(&self) -> bool {
        self.0.token_of(K::UntilKw).is_some()
    }

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
    /// Whether it ends `until`, which loops while the condition is false (FTEF-18.31).
    #[must_use]
    pub fn until(&self) -> bool {
        self.0.token_of(K::UntilKw).is_some()
    }

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

impl LabelStmt {
    /// The label's name (FTEF-18.23).
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }
}

impl GotoStmt {
    /// The name of the label it jumps to (FTEF-18.23).
    #[must_use]
    pub fn label(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }
}

impl AsmStmt {
    /// The `local` declarations among its instructions (FTEF-20.5).
    pub fn locals(&self) -> impl Iterator<Item = LocalDecl> {
        children(&self.0)
    }
}

impl LoopStmt {
    /// The loop body (FTEF-20.5).
    #[must_use]
    pub fn body(&self) -> Option<Stmt> {
        child(&self.0)
    }
}

impl ThinktimeStmt {
    /// The entity, then the time, in order (FTEF-20.5).
    pub fn operands(&self) -> impl Iterator<Item = Expr> {
        children(&self.0)
    }
}

impl StateStmt {
    /// The `[frame, next]` it runs (FTEF-20.5).
    #[must_use]
    pub fn state(&self) -> Option<State> {
        child(&self.0)
    }
}

impl UsingStmt {
    /// The names it aliases, in order (FTEF-20.5).
    pub fn aliases(&self) -> impl Iterator<Item = UsingAlias> {
        children(&self.0)
    }

    /// The statement the aliases are made for, when the names are in parentheses; without, they
    /// hold to the end of the scope.
    #[must_use]
    pub fn body(&self) -> Option<Stmt> {
        child(&self.0)
    }
}

impl UsingAlias {
    /// The name of the definition aliased (FTEF-20.5).
    #[must_use]
    pub fn target(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }

    /// The alias's name: the one after `=`, else the definition's own.
    #[must_use]
    pub fn alias(&self) -> Option<SyntaxToken> {
        self.0.child_tokens().filter(|t| t.kind() == K::Name).last()
    }
}

impl SizeofExpr {
    /// The type it measures, when it is given a type (FTEF-18.12).
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// The expression it measures, when it is given one (FTEF-18.12).
    #[must_use]
    pub fn operand(&self) -> Option<Expr> {
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
    /// The name: for `Scope::member` (FTEF-16.4), the member's.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        scoped_name(&self.0)
    }

    /// The scope of `Scope::member` (FTEF-16.4); `None` for a plain name.
    #[must_use]
    pub fn scope(&self) -> Option<SyntaxToken> {
        scope_of(&self.0)
    }
}

impl PointerType {
    /// The pointed-to type.
    #[must_use]
    pub fn inner(&self) -> Option<Type> {
        child(&self.0)
    }
}

impl VectorExpr {
    /// The component expressions, in order.
    pub fn elements(&self) -> impl Iterator<Item = Expr> {
        children(&self.0)
    }
}

impl ClassDecl {
    /// The class's name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }

    /// The parent class's name, after `:` (FTEF-9.1).
    #[must_use]
    pub fn parent(&self) -> Option<SyntaxToken> {
        self.0
            .child_tokens()
            .skip_while(|t| t.kind() != K::Colon)
            .find(|t| t.kind() == K::Name)
    }

    /// Whether the class has a body, so it is no forward declaration.
    #[must_use]
    pub fn has_body(&self) -> bool {
        self.0.token_of(K::LBrace).is_some()
    }

    /// The member declarations, in order (FTEF-9.2, FTEF-9.3).
    pub fn members(&self) -> impl Iterator<Item = Declaration> {
        children(&self.0)
    }

    /// The `get` and `set` members (FTEF-10.3).
    pub fn accessor_members(&self) -> impl Iterator<Item = AccessorMember> {
        children(&self.0)
    }

    /// The variables declared with the class as their type, `class C { ... } a, *b;`
    /// (FTEF-9.8).
    pub fn declarators(&self) -> impl Iterator<Item = Declarator> {
        children(&self.0)
    }
}

impl StructDecl {
    /// The variables `struct { ... } a, *b;` declares (FTEF-6.1).
    pub fn declarators(&self) -> impl Iterator<Item = Declarator> {
        children(&self.0)
    }

    /// The struct's tag.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children()
            .find(|n| n.kind() == K::StructType)
            .and_then(|n| n.token_of(K::Name))
    }

    /// The field nodes, in order.
    pub fn fields(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.descendants().filter(|n| n.kind() == K::StructField)
    }
}

impl AccessorDecl {
    /// The accessor type's name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.token_of(K::Name)
    }

    /// The members, in order.
    pub fn members(&self) -> impl Iterator<Item = AccessorMember> {
        children(&self.0)
    }

    /// The variables declared with the accessor as their type (FTEF-10.1).
    pub fn declarators(&self) -> impl Iterator<Item = Declarator> {
        children(&self.0)
    }
}

impl AccessorMember {
    /// The words before the type, `inline` and `get` or `set`, as the parser took them, in
    /// any case and in a class with a `__` (FTEF-20.11), and the names after them.
    fn words(&self) -> (Vec<SyntaxToken>, Vec<SyntaxToken>) {
        let names: Vec<SyntaxToken> = self
            .0
            .child_tokens()
            .filter(|t| t.kind() == K::Name)
            .collect();
        let is = |token: &SyntaxToken, word: &str| {
            let text = token.text_lossless();
            text.strip_prefix("__")
                .unwrap_or(&text)
                .eq_ignore_ascii_case(word)
        };
        let mut lead = 0_usize;
        if names.first().is_some_and(|t| is(t, "inline")) {
            lead = 1;
        }
        if names
            .get(lead)
            .is_some_and(|t| is(t, "get") || is(t, "set"))
        {
            lead = lead.saturating_add(1);
        }
        let (words, rest) = names.split_at(lead.min(names.len()));
        (words.to_vec(), rest.to_vec())
    }

    /// The member's name, if it has one: the name after `inline`, `get` or `set` and the
    /// type.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.words().1.into_iter().next()
    }

    /// The member's type, before its name (FTEF-10.3).
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// Whether it is a `set` member.
    #[must_use]
    pub fn is_set(&self) -> bool {
        self.words().0.last().is_some_and(|t| {
            let text = t.text_lossless();
            text.strip_prefix("__")
                .unwrap_or(&text)
                .eq_ignore_ascii_case("set")
        })
    }

    /// The index parameter's type in `[Type name]`, if any.
    #[must_use]
    pub fn index_ty(&self) -> Option<Type> {
        self.0
            .children()
            .find(|n| n.kind() == K::ArraySuffix)
            .and_then(|n| n.children().find_map(Type::cast))
    }

    /// The index parameter's name in `[Type name]`, if any.
    #[must_use]
    pub fn index_name(&self) -> Option<SyntaxToken> {
        self.0
            .children()
            .find(|n| n.kind() == K::ArraySuffix)
            .and_then(|n| n.token_of(K::Name))
    }

    /// The body of `= { ... }`.
    #[must_use]
    pub fn body(&self) -> Option<FunctionBody> {
        child(&self.0)
    }

    /// The value of `= function`.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
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
    /// `,` (FTEQCC, FTEF-18.5): both sides evaluated, the right one kept.
    Comma,
    /// `*^`, or `**` under FTEQCC's `ssp` flag (FTEQCC, FTEF-13.24): a power.
    Pow,
    /// `><` (FTEQCC, FTEF-13.24): a vector cross product.
    Cross,
    /// `<=>` (FTEQCC, FTEF-13.24): a three-way comparison.
    Spaceship,
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
            K::Comma => Self::Comma,
            K::StarCaret => Self::Pow,
            K::Cross => Self::Cross,
            K::Spaceship => Self::Spaceship,
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
    /// The arguments, in order; a named field's are [`ArgList::named`].
    pub fn args(&self) -> impl Iterator<Item = Expr> {
        children(&self.0)
    }

    /// The named fields of a `spawn` call, in order (FTEF-17.20).
    pub fn named(&self) -> impl Iterator<Item = NamedArg> {
        children(&self.0)
    }
}

impl NamedArg {
    /// The field's name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0.child_tokens().find(|t| !t.kind().is_trivia())
    }

    /// Whether `=` separates the name and the value, where `:` is expected.
    #[must_use]
    pub fn equals(&self) -> bool {
        self.0.child_tokens().any(|t| t.kind() == K::Eq)
    }

    /// The value the field is given.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

impl ForStmt {
    /// The three header clauses in order: init, cond and step (FTE-2.1). A clause that was
    /// empty has no node, so fewer than three may be returned.
    pub fn clauses(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|n| n.kind() == K::ForClause)
    }

    /// The initialiser clause, before the header's first `;`; `None` when it is empty.
    #[must_use]
    pub fn init(&self) -> Option<SyntaxNode> {
        self.clause(0)
    }

    /// The condition clause, between the header's two `;`; `None` when it is empty.
    #[must_use]
    pub fn condition(&self) -> Option<SyntaxNode> {
        self.clause(1)
    }

    /// The step clause, after the header's second `;`; `None` when it is empty.
    #[must_use]
    pub fn step(&self) -> Option<SyntaxNode> {
        self.clause(2)
    }

    /// The clause after `semicolons` of the header's `;`.
    fn clause(&self, semicolons: u8) -> Option<SyntaxNode> {
        let mut seen = 0_u8;
        for element in self.0.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind() == K::Semicolon => {
                    seen = seen.saturating_add(1);
                }
                SyntaxElement::Token(_) => {}
                SyntaxElement::Node(node) if node.kind() == K::ForClause && seen == semicolons => {
                    return Some(node);
                }
                SyntaxElement::Node(_) => {}
            }
        }
        None
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

    /// The body when it is no block, `switch (e) statement` (FTEF-18.33).
    #[must_use]
    pub fn statement(&self) -> Option<Stmt> {
        self.0.children().find_map(Stmt::cast)
    }
}

impl CaseLabel {
    /// The `case` value, or `None` for `default:` (FTE-3.1).
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The end of an FTEQCC case range, `case a .. b:` (FTEF-18.10).
    #[must_use]
    pub fn range_end(&self) -> Option<Expr> {
        around(&self.0, K::DotDot).1
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

    /// The arm chosen when the condition holds, and the arm chosen otherwise (FTE-8.1). FTEQCC's
    /// `a ?: b` has no middle arm: the condition's value is chosen (FTEF-18.11).
    #[must_use]
    pub fn arms(&self) -> (Option<Expr>, Option<Expr>) {
        let mut question = false;
        let mut colon = false;
        let mut middle = None;
        let mut otherwise = None;
        for element in self.0.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind() == K::Question => question = true,
                SyntaxElement::Token(token) if token.kind() == K::Colon => colon = question,
                SyntaxElement::Token(_) => {}
                SyntaxElement::Node(node) => {
                    if colon {
                        otherwise = otherwise.or_else(|| Expr::cast(node));
                    } else if question {
                        middle = middle.or_else(|| Expr::cast(node));
                    }
                }
            }
        }
        (middle, otherwise)
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

/// A prefix operator of FTEQCC (FTE-19.4, FTEF-16.7, FTEF-19.26).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `-`: negation.
    Neg,
    /// `+`: the operand unchanged.
    Plus,
    /// `~`: bitwise not.
    BitNot,
    /// `++e`: the operand incremented and stored (FTEF-16.7).
    PreIncrement,
    /// `--e`: the operand decremented and stored (FTEF-16.7).
    PreDecrement,
    /// `&e`: the operand's address (FTEF-16.7).
    AddressOf,
    /// `*e`: what the operand points at (FTEF-16.7).
    Deref,
    /// `@e` under `qccx`: the operand read as a string (FTEF-19.26).
    AsString,
}

impl UnaryExpr {
    /// The operator.
    #[must_use]
    pub fn op(&self) -> Option<UnaryOp> {
        self.0.child_tokens().find_map(|t| match t.kind() {
            K::Minus => Some(UnaryOp::Neg),
            K::Plus => Some(UnaryOp::Plus),
            K::Tilde => Some(UnaryOp::BitNot),
            K::PlusPlus => Some(UnaryOp::PreIncrement),
            K::MinusMinus => Some(UnaryOp::PreDecrement),
            K::Amp => Some(UnaryOp::AddressOf),
            K::Star => Some(UnaryOp::Deref),
            K::At => Some(UnaryOp::AsString),
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
    /// The declarators, in order: each defines a type (FTEF-7.3).
    pub fn declarators(&self) -> impl Iterator<Item = Declarator> {
        children(&self.0)
    }
}

impl EnumDecl {
    /// The enum's type name, `enum Name { ... }` or `enum class Name ...` (FTE-15.4); `None`
    /// for an anonymous enum.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        // The base type after `:` and the enumerators after `{` are nodes of their own, so
        // the enum's own tokens hold no other name.
        self.0
            .child_tokens()
            .filter(|t| t.kind() == K::Name)
            .find(|t| t.text_lossless() != "class")
    }

    /// The enumerators, in order (FTE-15.1).
    pub fn enumerators(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|n| n.kind() == K::Enumerator)
    }

    /// The variables `enum E { ... } a, *b;` declares at file level (FTEF-8.5).
    pub fn declarators(&self) -> impl Iterator<Item = Declarator> {
        children(&self.0)
    }
}
