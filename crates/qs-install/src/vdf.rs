// SPDX-License-Identifier: GPL-3.0-or-later

//! Steam's text KeyValues files: `libraryfolders.vdf` and `appmanifest_<id>.acf`.
//!
//! The format, as Steam writes these two files: quoted keys and values, `{` and `}` around
//! a nested block, `//` comments to the end of the line, and `\\`, `\"`, `\n` and `\t`
//! escapes inside quotes. Keys are compared ASCII case-insensitively, since Steam has
//! written both `LibraryFolders` and `libraryfolders`.

/// A value: a string or a block of key-value pairs in file order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    Str(String),
    Block(Vec<(String, Value)>),
}

impl Value {
    /// The first value under `key` in a block.
    pub(crate) fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Block(pairs) => pairs
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value),
            Self::Str(_) => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            Self::Block(_) => None,
        }
    }

    pub(crate) fn pairs(&self) -> &[(String, Self)] {
        match self {
            Self::Block(pairs) => pairs,
            Self::Str(_) => &[],
        }
    }
}

/// Deeper nesting than this is rejected, so a hostile file cannot exhaust the stack.
const MAX_DEPTH: usize = 32;

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Text(String),
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                let mut value = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            'n' => value.push('\n'),
                            't' => value.push('\t'),
                            other => value.push(other),
                        },
                        other => value.push(other),
                    }
                }
                tokens.push(Token::Text(value));
            }
            c if c.is_whitespace() => {}
            c => {
                let mut value = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '{' | '}' | '"') {
                        break;
                    }
                    value.push(next);
                    chars.next();
                }
                tokens.push(Token::Text(value));
            }
        }
    }
    Some(tokens)
}

/// Parses a whole file into its top-level block. Returns `None` for anything malformed: an
/// unterminated string, an unbalanced brace, a key without a value, or nesting deeper than
/// [`MAX_DEPTH`].
pub(crate) fn parse(text: &str) -> Option<Value> {
    let tokens = tokenize(text)?;
    let mut iter = tokens.into_iter();
    let pairs = parse_block(&mut iter, 0, false)?;
    Some(Value::Block(pairs))
}

fn parse_block(
    tokens: &mut impl Iterator<Item = Token>,
    depth: usize,
    nested: bool,
) -> Option<Vec<(String, Value)>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut pairs = Vec::new();
    loop {
        let key = match tokens.next() {
            None => return (!nested).then_some(pairs),
            Some(Token::Close) => return nested.then_some(pairs),
            Some(Token::Open) => return None,
            Some(Token::Text(key)) => key,
        };
        let value = match tokens.next()? {
            Token::Text(value) => Value::Str(value),
            Token::Open => Value::Block(parse_block(tokens, depth.saturating_add(1), true)?),
            Token::Close => return None,
        };
        pairs.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use proptest::prelude::*;

    use super::*;

    #[test]
    fn parses_nested_blocks_comments_and_escapes() {
        let text = "// header\n\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"apps\" { \"2310\" \"123\" }\n\t}\n}\n";
        let value = parse(text).unwrap();
        let library = value.get("LibraryFolders").unwrap().get("0").unwrap();
        assert_eq!(
            library.get("path").and_then(Value::as_str),
            Some(r"C:\Program Files (x86)\Steam")
        );
        assert_eq!(
            library
                .get("apps")
                .and_then(|apps| apps.get("2310"))
                .and_then(Value::as_str),
            Some("123")
        );
    }

    #[test]
    fn accepts_unquoted_tokens_and_tab_and_newline_escapes() {
        let value = parse("AppState { installdir Quake note \"a\\tb\\nc\\\"\" }").unwrap();
        let state = value.get("appstate").unwrap();
        assert_eq!(
            state.get("installdir").and_then(Value::as_str),
            Some("Quake")
        );
        assert_eq!(state.get("note").and_then(Value::as_str), Some("a\tb\nc\""));
        assert_eq!(value.pairs().len(), 1);
        assert!(state.as_str().is_none());
        assert!(Value::Str(String::new()).get("x").is_none());
        assert!(Value::Str(String::new()).pairs().is_empty());
    }

    #[test]
    fn a_single_slash_is_not_a_comment() {
        let value = parse("path /data/steam // the library").unwrap();
        assert_eq!(
            value.get("path").and_then(Value::as_str),
            Some("/data/steam")
        );
        assert_eq!(value.pairs().len(), 1);
    }

    #[test]
    fn empty_input_is_an_empty_block() {
        assert_eq!(parse(""), Some(Value::Block(Vec::new())));
        assert_eq!(parse("  // only a comment"), Some(Value::Block(Vec::new())));
    }

    #[test]
    fn rejects_malformed_files() {
        for text in [
            "\"unterminated",
            "\"a\" \"b\\",
            "\"key\"",
            "\"a\" {",
            "\"a\" { \"b\" \"c\" } }",
            "}",
            "{ }",
            "\"a\" }",
            "\"a\" { \"b\" }",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn nesting_is_limited() {
        let deep = |levels: usize| "\"k\" {".repeat(levels) + &"}".repeat(levels);
        assert!(parse(&deep(MAX_DEPTH)).is_some());
        assert_eq!(parse(&deep(MAX_DEPTH + 1)), None);
    }

    proptest! {
        #[test]
        fn never_panics(text in "[\\PC{}\"\\\\/ \n]{0,200}") {
            let _parsed = parse(&text);
        }
    }
}
