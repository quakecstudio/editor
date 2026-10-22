// SPDX-License-Identifier: GPL-3.0-or-later

//! What the features read from a file's tree as written: the token at a span, the declaration a
//! name is in, its comments and its parameter names.
//!
//! The semantic model resolves a module through the preprocessor, but reports every location
//! at a span of the file as written (QC-9.7), so the tree of the file as written
//! (`qc_sema::Db::parse`) has a token at every location a name is written in it. A location
//! that came from a macro expansion is the span of the macro's use (QC-9.7), whose text is not
//! the name; [`name_at`] tells the two apart.

use qc_diag::Span;
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::{Parse, SyntaxKind, SyntaxNode, SyntaxToken};

/// The token of `parse` whose span is `span`, if any.
pub(crate) fn token_at(parse: &Parse, span: Span) -> Option<SyntaxToken> {
    parse.syntax().tokens().find(|t| t.span() == span)
}

/// The name token covering byte `offset`, or ending at it (a cursor just after a name).
pub(crate) fn name_token_near(parse: &Parse, offset: usize) -> Option<SyntaxToken> {
    let mut before = None;
    for token in parse.syntax().tokens() {
        let span = token.span();
        if span.start > offset {
            break;
        }
        if token.kind() == SyntaxKind::Name {
            if offset < span.end {
                return Some(token);
            }
            if offset == span.end {
                before = Some(token);
            }
        }
    }
    before
}

/// Whether the text of `text` at `span` is exactly `name`: a location written in the file,
/// not the use of a macro that expanded to it.
pub(crate) fn name_at(text: &[u8], span: Span, name: &str) -> bool {
    text.get(span.start..span.end) == Some(name.as_bytes())
}

/// The node a token is in, then that node's ancestors to the root.
fn enclosing(token: &SyntaxToken) -> impl Iterator<Item = SyntaxNode> {
    std::iter::successors(Some(token.parent()), SyntaxNode::parent)
}

/// The declaration a name token is in: a file-level declaration, a `local`, or a parameter.
pub(crate) fn declaration_of(token: &SyntaxToken) -> Option<SyntaxNode> {
    enclosing(token).find(|n| {
        matches!(
            n.kind(),
            SyntaxKind::Declaration | SyntaxKind::LocalDecl | SyntaxKind::Param
        )
    })
}

/// How the name `name` of declaration `decl` was declared, white space folded to single
/// spaces: its type, then the name, then an FTEQCC C-style parameter list (FTE-17.1), with
/// `local` before a local, such as `float(entity targ) T_Heal`. `None` when the declaration
/// has no type.
pub(crate) fn declared_as(decl: &SyntaxNode, name: &SyntaxToken) -> Option<String> {
    let (local, ty, params) = match decl.kind() {
        SyntaxKind::Declaration => {
            let d = ast::Declaration::cast(decl.clone())?;
            let params = d
                .declarators()
                .find(|dr| dr.name().is_some_and(|n| n.span() == name.span()))
                .and_then(|dr| dr.params());
            (false, d.ty()?, params)
        }
        SyntaxKind::LocalDecl => (true, ast::LocalDecl::cast(decl.clone())?.ty()?, None),
        SyntaxKind::Param => (false, ast::Param::cast(decl.clone())?.ty()?, None),
        _ => return None,
    };
    let mut out = String::new();
    if local {
        out.push_str("local ");
    }
    out.push_str(&folded(ty.syntax()));
    out.push(' ');
    out.push_str(&name.text_lossless());
    if let Some(params) = params {
        out.push_str(&folded(params.syntax()));
    }
    Some(out)
}

/// A node's text with its comments dropped and white space folded to single spaces, none at
/// either end.
fn folded(node: &SyntaxNode) -> String {
    let mut out = String::new();
    let mut space = false;
    for token in node.tokens() {
        if is_trivia(token.kind()) {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push_str(&token.text_lossless());
        }
    }
    out
}

/// The comments that document the declaration `decl`: the comments on the lines right above
/// it, each starting its line, with no blank line between, else the comment after it on its
/// last line.
pub(crate) fn doc_comment(parse: &Parse, decl: &SyntaxNode) -> Option<String> {
    let tokens: Vec<SyntaxToken> = parse.syntax().tokens().collect();
    let code: Vec<SyntaxToken> = decl.tokens().filter(|t| !is_trivia(t.kind())).collect();
    let first = tokens.iter().position(|t| Some(t) == code.first())?;
    let last = tokens.iter().position(|t| Some(t) == code.last())?;
    let mut above = Vec::new();
    let mut at = first;
    while let Some(index) = at.checked_sub(1) {
        at = index;
        let Some(token) = tokens.get(index) else {
            break;
        };
        match token.kind() {
            SyntaxKind::Whitespace if line_breaks(token) > 1 => break,
            SyntaxKind::Whitespace => {}
            SyntaxKind::LineComment | SyntaxKind::BlockComment if starts_line(&tokens, index) => {
                above.push(comment_text(token));
            }
            _ => break,
        }
    }
    above.reverse();
    let joined = above.join("\n");
    // A banner of empty comment lines around the text is not part of it.
    let joined = joined.trim();
    if !joined.is_empty() {
        return Some(joined.to_owned());
    }
    for token in tokens.get(last.checked_add(1)?..)? {
        match token.kind() {
            SyntaxKind::LineComment | SyntaxKind::BlockComment => {
                let text = comment_text(token);
                return (!text.is_empty()).then_some(text);
            }
            SyntaxKind::Whitespace if line_breaks(token) == 0 => {}
            _ => return None,
        }
    }
    None
}

/// Whether a token of this kind is white space or a comment.
fn is_trivia(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Whitespace | SyntaxKind::LineComment | SyntaxKind::BlockComment
    )
}

/// How many line breaks a token's text holds.
fn line_breaks(token: &SyntaxToken) -> usize {
    token.text().iter().filter(|&&b| b == b'\n').count()
}

/// Whether the token at `index` starts its line: it is the first token, or white space with a
/// line break is before it.
fn starts_line(tokens: &[SyntaxToken], index: usize) -> bool {
    let Some(before) = index.checked_sub(1) else {
        return true;
    };
    tokens
        .get(before)
        .is_some_and(|t| t.kind() == SyntaxKind::Whitespace && line_breaks(t) > 0)
}

/// A comment's text without its markers and the white space around it.
fn comment_text(token: &SyntaxToken) -> String {
    let text = token.text_lossless();
    let inner = text
        .strip_prefix("//")
        .or_else(|| {
            text.strip_prefix("/*")
                .map(|t| t.strip_suffix("*/").unwrap_or(t))
        })
        .unwrap_or(&text);
    inner.trim().to_owned()
}

/// Whether the declarator of `name` in declaration `decl` has a function body.
pub(crate) fn has_body(decl: &SyntaxNode, name: &SyntaxToken) -> bool {
    ast::Declaration::cast(decl.clone()).is_some_and(|d| {
        d.declarators().any(|dr| {
            dr.name().is_some_and(|n| n.span() == name.span())
                && matches!(dr.initializer(), Some(ast::Initializer::Body(_)))
        })
    })
}

/// The names of the parameters of the function a declaration declares, in order, `None` for
/// a parameter with no name; empty when it declares no function.
pub(crate) fn parameter_names(decl: &SyntaxNode, name: &SyntaxToken) -> Vec<Option<String>> {
    let list = ast::Declaration::cast(decl.clone()).and_then(|d| {
        // An FTEQCC C-style declarator has its own list (FTE-17.1).
        let declarator = d
            .declarators()
            .find(|dr| dr.name().is_some_and(|n| n.span() == name.span()));
        declarator
            .and_then(|dr| dr.params())
            .or_else(|| match d.ty() {
                Some(ast::Type::Function(f)) => f.params(),
                Some(ast::Type::Simple(_) | ast::Type::Field(_) | ast::Type::Pointer(_)) | None => {
                    None
                }
            })
    });
    list.map(|l| {
        l.params()
            .map(|p| p.name().map(|n| n.text_lossless()))
            .collect()
    })
    .unwrap_or_default()
}

/// The name token of the function whose body holds byte `offset`, if any.
pub(crate) fn enclosing_function(parse: &Parse, offset: usize) -> Option<SyntaxToken> {
    let root = parse.syntax();
    let token = root
        .tokens()
        .take_while(|t| t.span().start <= offset)
        .last()?;
    enclosing(&token)
        .filter_map(ast::Declarator::cast)
        .find(|d| matches!(d.initializer(), Some(ast::Initializer::Body(_))))
        .and_then(|d| d.name())
}

/// Whether the token before the name at `offset`, or before `offset` itself, is a member
/// access: `.` or `->`.
pub(crate) fn after_member_access(parse: &Parse, offset: usize) -> bool {
    let mut previous = None;
    for token in parse.syntax().tokens() {
        let span = token.span();
        if span.start >= offset {
            break;
        }
        // The name being typed is not what comes before it.
        if token.kind() == SyntaxKind::Name && span.end >= offset {
            break;
        }
        if !is_trivia(token.kind()) {
            previous = Some(token.kind());
        }
    }
    matches!(previous, Some(SyntaxKind::Dot | SyntaxKind::Arrow))
}
