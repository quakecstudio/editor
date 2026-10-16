// SPDX-License-Identifier: GPL-3.0-or-later

//! The tree types: identity, hashing, display, and the builder's edge cases.

use std::collections::HashSet;
use std::hash::{BuildHasher as _, BuildHasherDefault, DefaultHasher, Hash};

use qc_lexer::Code;

use crate::tree::Builder;
use crate::{GreenElement, GreenNode, GreenToken, SyntaxKind, SyntaxNode, parse};

#[test]
fn nodes_compare_and_hash_by_position() {
    let root = parse(b"float a; float a;").syntax();
    let decls: Vec<SyntaxNode> = root.children().collect();
    assert_ne!(decls[0], decls[1]);
    assert_eq!(decls[0].green(), decls[1].green());
    assert_eq!(decls[0], root.children().next().unwrap());
    let other = parse(b"float a; float a;").syntax();
    assert_ne!(root, other, "the same text in another tree is another node");
    let set: HashSet<SyntaxNode> = root.children().chain(root.children()).collect();
    assert_eq!(set.len(), 2);
}

#[test]
fn tokens_compare_and_hash_by_position() {
    let root = parse(b"float a; float a;").syntax();
    let names: Vec<_> = root
        .tokens()
        .filter(|t| t.kind() == SyntaxKind::Name)
        .collect();
    assert_ne!(names[0], names[1]);
    assert_eq!(names[0].text(), names[1].text());
    let again: Vec<_> = root
        .tokens()
        .filter(|t| t.kind() == SyntaxKind::Name)
        .collect();
    assert_eq!(names, again);
    let other = parse(b"float a; float a;").syntax();
    let first_other = other.tokens().find(|t| t.kind() == SyntaxKind::Name);
    assert_ne!(Some(names[0].clone()), first_other);
    let set: HashSet<_> = names.iter().chain(again.iter()).cloned().collect();
    assert_eq!(set.len(), 2);
}

fn hash_of(value: &impl Hash) -> u64 {
    BuildHasherDefault::<DefaultHasher>::default().hash_one(value)
}

#[test]
fn hashes_follow_position() {
    let root = parse(b"float a; float a;").syntax();
    let decls: Vec<SyntaxNode> = root.children().collect();
    assert_eq!(
        hash_of(&decls[0]),
        hash_of(&root.children().next().unwrap())
    );
    assert_ne!(hash_of(&decls[0]), hash_of(&decls[1]));
    let names: Vec<_> = root
        .tokens()
        .filter(|t| t.kind() == SyntaxKind::Name)
        .collect();
    assert_eq!(hash_of(&names[0]), hash_of(&names[0].clone()));
    assert_ne!(hash_of(&names[0]), hash_of(&names[1]));
}

#[test]
fn debug_formats() {
    let root = parse(b"float a;").syntax();
    assert_eq!(format!("{root:?}"), "SourceFile@0..8");
    let decl = root.children().next().unwrap();
    assert_eq!(format!("{decl:?}"), "Declaration@0..8");
    let semi = decl.token_of(SyntaxKind::Semicolon).unwrap();
    assert_eq!(format!("{semi:?}"), "Semicolon@7..8 \";\"");
}

#[test]
fn green_lengths() {
    let empty = GreenNode::new(SyntaxKind::Error, Vec::new());
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    let token = GreenToken::new(SyntaxKind::Name, b"abc");
    let node = GreenNode::new(
        SyntaxKind::NameRef,
        vec![GreenElement::Token(token.clone())],
    );
    assert!(!node.is_empty());
    assert_eq!(node.len(), 3);
    assert!(!GreenElement::Token(token).is_empty());
    let blank = GreenElement::Token(GreenToken::new(SyntaxKind::Whitespace, b""));
    assert!(blank.is_empty());
    assert!(GreenElement::Node(empty).is_empty());
    assert_eq!(GreenElement::Node(node).kind(), SyntaxKind::NameRef);
}

#[test]
fn builder_wraps_a_root_that_is_not_a_source_file() {
    let mut builder = Builder::default();
    builder.token(SyntaxKind::Name, b"a");
    builder.token(SyntaxKind::Whitespace, b" ");
    let root = builder.finish();
    assert_eq!(root.kind(), SyntaxKind::SourceFile);
    assert_eq!(root.children().len(), 2);

    let mut builder = Builder::default();
    builder.start_node(SyntaxKind::Declaration);
    builder.token(SyntaxKind::Name, b"a");
    let root = builder.finish();
    assert_eq!(root.kind(), SyntaxKind::SourceFile);
    assert_eq!(root.children()[0].kind(), SyntaxKind::Declaration);

    let mut builder = Builder::default();
    builder.start_node(SyntaxKind::SourceFile);
    builder.token(SyntaxKind::Name, b"a");
    builder.finish_node();
    builder.token(SyntaxKind::Name, b"b");
    let root = builder.finish();
    assert_eq!(root.kind(), SyntaxKind::SourceFile);
    assert_eq!(root.children().len(), 2);

    let mut builder = Builder::default();
    builder.start_node(SyntaxKind::SourceFile);
    builder.finish_node();
    builder.finish_node();
    let root = builder.finish();
    assert_eq!(root.kind(), SyntaxKind::SourceFile);
    assert!(root.children().is_empty());
}

#[test]
fn keyword_and_trivia_kinds() {
    let keywords = [
        SyntaxKind::FloatKw,
        SyntaxKind::VectorKw,
        SyntaxKind::StringKw,
        SyntaxKind::EntityKw,
        SyntaxKind::VoidKw,
        SyntaxKind::LocalKw,
        SyntaxKind::ReturnKw,
        SyntaxKind::IfKw,
        SyntaxKind::ElseKw,
        SyntaxKind::WhileKw,
        SyntaxKind::DoKw,
    ];
    for kind in keywords {
        assert!(kind.is_keyword(), "{kind:?}");
        assert!(!kind.is_trivia(), "{kind:?}");
    }
    for kind in [
        SyntaxKind::Name,
        SyntaxKind::Semicolon,
        SyntaxKind::Declaration,
    ] {
        assert!(!kind.is_keyword(), "{kind:?}");
    }
}

// spec QCD-1.4: the parse keeps the lexer's diagnostics, warnings included, in source order
#[test]
fn diagnostics_include_lexical_ones_and_warnings() {
    let long = "f".repeat(16);
    let source = format!("$frame {long}\nfloat b c;\nstring a = \"a\\tb\";");
    let parsed = parse(source.as_bytes());
    let all: Vec<Code> = parsed.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(all, vec![Code::W0101, Code::E0204, Code::E0103]);
    let errors: Vec<Code> = parsed.errors().map(|d| d.code).collect();
    assert_eq!(errors, vec![Code::E0204, Code::E0103]);
}
