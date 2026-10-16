// SPDX-License-Identifier: GPL-3.0-or-later

//! The concrete syntax tree, in the two layers rust-analyzer's `rowan` uses.
//!
//! The green tree is immutable and position-free: a node holds its kind, its length and its
//! children, a token its kind and its bytes, so identical subtrees can be shared and the
//! tree can be sent between threads. The red tree ([`SyntaxNode`], [`SyntaxToken`]) is a
//! cursor over it built on demand, which knows each element's parent and byte offset.
//!
//! Tokens hold bytes, not `str`: QuakeC source has no encoding (QC-1.1), and the tree must
//! give back every byte of it.

use std::fmt::{self, Write as _};
use std::rc::Rc;
use std::sync::Arc;

use qc_lexer::Span;

use crate::kind::SyntaxKind;

#[derive(Debug, PartialEq, Eq, Hash)]
struct GreenTokenData {
    kind: SyntaxKind,
    text: Box<[u8]>,
}

/// A token of the green tree: a kind and its bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GreenToken(Arc<GreenTokenData>);

impl GreenToken {
    /// A token of `kind` with the bytes `text`.
    #[must_use]
    pub fn new(kind: SyntaxKind, text: &[u8]) -> Self {
        Self(Arc::new(GreenTokenData {
            kind,
            text: text.into(),
        }))
    }

    /// Its kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind
    }

    /// Its bytes.
    #[must_use]
    pub fn text(&self) -> &[u8] {
        &self.0.text
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct GreenNodeData {
    kind: SyntaxKind,
    len: usize,
    children: Box<[GreenElement]>,
}

/// A node of the green tree: a kind, its length in bytes and its children.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GreenNode(Arc<GreenNodeData>);

impl GreenNode {
    /// A node of `kind` holding `children`.
    #[must_use]
    pub fn new(kind: SyntaxKind, children: Vec<GreenElement>) -> Self {
        let len = children
            .iter()
            .fold(0_usize, |len, child| len.saturating_add(child.len()));
        Self(Arc::new(GreenNodeData {
            kind,
            len,
            children: children.into_boxed_slice(),
        }))
    }

    /// Its kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind
    }

    /// Its length in bytes: the sum of its tokens' lengths.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len
    }

    /// Whether it holds no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.len == 0
    }

    /// Its children in order.
    #[must_use]
    pub fn children(&self) -> &[GreenElement] {
        &self.0.children
    }

    /// Appends every byte under it to `out`, in order.
    pub fn write_text(&self, out: &mut Vec<u8>) {
        for child in self.children() {
            match child {
                GreenElement::Node(node) => node.write_text(out),
                GreenElement::Token(token) => out.extend_from_slice(token.text()),
            }
        }
    }
}

/// A child of a green node.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GreenElement {
    /// A node.
    Node(GreenNode),
    /// A token.
    Token(GreenToken),
}

impl GreenElement {
    /// Its length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Node(node) => node.len(),
            Self::Token(token) => token.text().len(),
        }
    }

    /// Whether it holds no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Its kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        match self {
            Self::Node(node) => node.kind(),
            Self::Token(token) => token.kind(),
        }
    }
}

/// A position in a [`Builder`] where a node can later be started around what follows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Checkpoint(usize);

/// Builds a green tree from a stream of tokens and node boundaries.
#[derive(Debug, Default)]
pub(crate) struct Builder {
    parents: Vec<(SyntaxKind, usize)>,
    children: Vec<GreenElement>,
}

impl Builder {
    /// Adds a token to the current node.
    pub(crate) fn token(&mut self, kind: SyntaxKind, text: &[u8]) {
        self.children
            .push(GreenElement::Token(GreenToken::new(kind, text)));
    }

    /// Starts a node; what is added until the matching [`Builder::finish_node`] goes in it.
    pub(crate) fn start_node(&mut self, kind: SyntaxKind) {
        self.parents.push((kind, self.children.len()));
    }

    /// The current position, to start a node there later.
    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint(self.children.len())
    }

    /// Starts a node that takes in everything added since `checkpoint`.
    pub(crate) fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        let floor = self.parents.last().map_or(0, |&(_, first)| first);
        let at = checkpoint.0.clamp(floor, self.children.len());
        self.parents.push((kind, at));
    }

    /// Finishes the innermost node.
    pub(crate) fn finish_node(&mut self) {
        let Some((kind, first)) = self.parents.pop() else {
            return;
        };
        let children = self.children.split_off(first.min(self.children.len()));
        self.children
            .push(GreenElement::Node(GreenNode::new(kind, children)));
    }

    /// Finishes every open node and returns the root, a [`SyntaxKind::SourceFile`] node.
    pub(crate) fn finish(mut self) -> GreenNode {
        while !self.parents.is_empty() {
            self.finish_node();
        }
        match self.children.pop() {
            Some(GreenElement::Node(root))
                if self.children.is_empty() && root.kind() == SyntaxKind::SourceFile =>
            {
                root
            }
            last => {
                self.children.extend(last);
                GreenNode::new(SyntaxKind::SourceFile, self.children)
            }
        }
    }
}

#[derive(Debug)]
struct NodeData {
    green: GreenNode,
    parent: Option<SyntaxNode>,
    offset: usize,
}

/// A node of the tree with its position: a cursor over a [`GreenNode`].
#[derive(Clone)]
pub struct SyntaxNode(Rc<NodeData>);

/// A token of the tree with its position.
#[derive(Clone)]
pub struct SyntaxToken {
    green: GreenToken,
    parent: SyntaxNode,
    offset: usize,
}

/// A child of a node: a node or a token.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SyntaxElement {
    /// A node.
    Node(SyntaxNode),
    /// A token.
    Token(SyntaxToken),
}

impl SyntaxElement {
    /// Its kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        match self {
            Self::Node(node) => node.kind(),
            Self::Token(token) => token.kind(),
        }
    }

    /// Its bytes in the source.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Node(node) => node.span(),
            Self::Token(token) => token.span(),
        }
    }
}

impl SyntaxNode {
    /// The root of the tree `green`, at offset 0.
    #[must_use]
    pub fn new_root(green: GreenNode) -> Self {
        Self(Rc::new(NodeData {
            green,
            parent: None,
            offset: 0,
        }))
    }

    /// Its kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.green.kind()
    }

    /// Its green node.
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.0.green
    }

    /// Its bytes in the source, trivia inside it included.
    #[must_use]
    pub fn span(&self) -> Span {
        Span::new(
            self.0.offset,
            self.0.offset.saturating_add(self.0.green.len()),
        )
    }

    /// The node it is in, if it is not the root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.0.parent.clone()
    }

    /// Its parent, that node's parent, and so on to the root.
    pub fn ancestors(&self) -> impl Iterator<Item = Self> {
        std::iter::successors(self.parent(), Self::parent)
    }

    /// Its children, nodes and tokens, in order.
    pub fn children_with_tokens(&self) -> impl Iterator<Item = SyntaxElement> {
        let parent = self.clone();
        let mut offset = self.0.offset;
        let green = self.0.green.clone();
        (0..green.children().len()).filter_map(move |index| {
            let child = green.children().get(index)?;
            let at = offset;
            offset = offset.saturating_add(child.len());
            Some(match child {
                GreenElement::Node(node) => SyntaxElement::Node(Self(Rc::new(NodeData {
                    green: node.clone(),
                    parent: Some(parent.clone()),
                    offset: at,
                }))),
                GreenElement::Token(token) => SyntaxElement::Token(SyntaxToken {
                    green: token.clone(),
                    parent: parent.clone(),
                    offset: at,
                }),
            })
        })
    }

    /// Its child nodes, in order.
    pub fn children(&self) -> impl Iterator<Item = Self> {
        self.children_with_tokens()
            .filter_map(|element| match element {
                SyntaxElement::Node(node) => Some(node),
                SyntaxElement::Token(_) => None,
            })
    }

    /// Its child tokens, in order.
    pub fn child_tokens(&self) -> impl Iterator<Item = SyntaxToken> {
        self.children_with_tokens()
            .filter_map(|element| match element {
                SyntaxElement::Token(token) => Some(token),
                SyntaxElement::Node(_) => None,
            })
    }

    /// It and every node under it, in source order.
    pub fn descendants(&self) -> impl Iterator<Item = Self> {
        self.descendants_with_tokens()
            .filter_map(|element| match element {
                SyntaxElement::Node(node) => Some(node),
                SyntaxElement::Token(_) => None,
            })
    }

    /// It and every node and token under it, in source order.
    pub fn descendants_with_tokens(&self) -> impl Iterator<Item = SyntaxElement> {
        let mut stack = vec![SyntaxElement::Node(self.clone())];
        std::iter::from_fn(move || {
            let element = stack.pop()?;
            if let SyntaxElement::Node(node) = &element {
                let children: Vec<SyntaxElement> = node.children_with_tokens().collect();
                stack.extend(children.into_iter().rev());
            }
            Some(element)
        })
    }

    /// Every token under it, trivia included, in source order.
    pub fn tokens(&self) -> impl Iterator<Item = SyntaxToken> {
        self.descendants_with_tokens()
            .filter_map(|element| match element {
                SyntaxElement::Token(token) => Some(token),
                SyntaxElement::Node(_) => None,
            })
    }

    /// Its first child token of `kind`.
    #[must_use]
    pub fn token_of(&self, kind: SyntaxKind) -> Option<SyntaxToken> {
        self.child_tokens().find(|token| token.kind() == kind)
    }

    /// Its source text: every byte under it, in order.
    #[must_use]
    pub fn text(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.0.green.len());
        self.0.green.write_text(&mut out);
        out
    }

    /// The tree under it, one element per line, for tests and debugging.
    #[must_use]
    pub fn debug_tree(&self) -> String {
        let mut out = String::new();
        self.write_tree(&mut out, 0);
        out
    }

    fn write_tree(&self, out: &mut String, depth: usize) {
        let indent = depth.saturating_mul(2);
        let span = self.span();
        writeln!(
            out,
            "{:indent$}{:?}@{}..{}",
            "",
            self.kind(),
            span.start,
            span.end
        )
        .unwrap_or(());
        let child_depth = depth.saturating_add(1);
        for child in self.children_with_tokens() {
            match child {
                SyntaxElement::Node(node) => node.write_tree(out, child_depth),
                SyntaxElement::Token(token) => {
                    let indent = child_depth.saturating_mul(2);
                    let span = token.span();
                    writeln!(
                        out,
                        "{:indent$}{:?}@{}..{} \"{}\"",
                        "",
                        token.kind(),
                        span.start,
                        span.end,
                        token.text().escape_ascii()
                    )
                    .unwrap_or(());
                }
            }
        }
    }
}

impl PartialEq for SyntaxNode {
    fn eq(&self, other: &Self) -> bool {
        self.0.offset == other.0.offset && Arc::ptr_eq(&self.0.green.0, &other.0.green.0)
    }
}

impl Eq for SyntaxNode {}

impl std::hash::Hash for SyntaxNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.offset.hash(state);
        Arc::as_ptr(&self.0.green.0).hash(state);
    }
}

impl fmt::Debug for SyntaxNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let span = self.span();
        write!(f, "{:?}@{}..{}", self.kind(), span.start, span.end)
    }
}

impl SyntaxToken {
    /// Its kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.green.kind()
    }

    /// Its bytes.
    #[must_use]
    pub fn text(&self) -> &[u8] {
        self.green.text()
    }

    /// Its bytes as a string, each byte one `char` (Latin-1), so nothing is lost.
    #[must_use]
    pub fn text_lossless(&self) -> String {
        self.text().iter().copied().map(char::from).collect()
    }

    /// Its bytes in the source.
    #[must_use]
    pub fn span(&self) -> Span {
        Span::new(
            self.offset,
            self.offset.saturating_add(self.green.text().len()),
        )
    }

    /// The node it is in.
    #[must_use]
    pub fn parent(&self) -> SyntaxNode {
        self.parent.clone()
    }
}

impl PartialEq for SyntaxToken {
    fn eq(&self, other: &Self) -> bool {
        self.offset == other.offset && Arc::ptr_eq(&self.green.0, &other.green.0)
    }
}

impl Eq for SyntaxToken {}

impl std::hash::Hash for SyntaxToken {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.offset.hash(state);
        Arc::as_ptr(&self.green.0).hash(state);
    }
}

impl fmt::Debug for SyntaxToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let span = self.span();
        write!(
            f,
            "{:?}@{}..{} \"{}\"",
            self.kind(),
            span.start,
            span.end,
            self.text().escape_ascii()
        )
    }
}
