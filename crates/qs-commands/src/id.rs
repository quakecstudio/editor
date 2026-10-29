// SPDX-License-Identifier: GPL-3.0-or-later

//! Stable command ids.

use std::borrow::Borrow;
use std::fmt;

use serde::Serialize;

/// The longest id, in bytes: the upper bound MCP gives tool names (MCP 2026-07-28, Tools, Tool
/// Names: "Tool names SHOULD be between 1 and 128 characters in length (inclusive)").
pub const MAX_ID_LEN: usize = 128;

/// A command's stable id, such as `edit.undo` or `compile`.
///
/// One or more segments joined by `.`; each segment is a lowercase ASCII letter followed by
/// lowercase ASCII letters, digits and `_`; at most [`MAX_ID_LEN`] bytes in all. Every such id
/// is also a valid MCP tool name, so the agent and MCP surfaces can use it unchanged.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct CommandId(String);

impl CommandId {
    /// The id `text` spells, or why it is not one.
    ///
    /// # Errors
    ///
    /// [`IdError`] when `text` breaks the grammar above.
    pub fn new(text: &str) -> Result<Self, IdError> {
        if text.is_empty() {
            return Err(IdError::Empty);
        }
        if text.len() > MAX_ID_LEN {
            return Err(IdError::TooLong { len: text.len() });
        }
        for segment in text.split('.') {
            let mut chars = segment.chars();
            let Some(first) = chars.next() else {
                return Err(IdError::EmptySegment);
            };
            if !first.is_ascii_lowercase() {
                return Err(IdError::BadSegmentStart { found: first });
            }
            let is_id_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
            if let Some(found) = chars.find(|&c| !is_id_char(c)) {
                return Err(IdError::BadCharacter { found });
            }
        }
        Ok(Self(text.to_owned()))
    }

    /// The id of a built-in command, which the crate's tests check against the grammar.
    pub(crate) fn builtin(text: &'static str) -> Self {
        Self(text.to_owned())
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lets a registry keyed by id be searched with the text a caller sends. `Ord` is derived from
/// the one `String` field, so it orders ids exactly as `str` orders their text, as `Borrow`
/// requires.
impl Borrow<str> for CommandId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// Why some text is not a [`CommandId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdError {
    /// The text is empty.
    Empty,
    /// The text is longer than [`MAX_ID_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// Two dots are adjacent, or the text starts or ends with one.
    EmptySegment,
    /// A segment starts with something other than a lowercase ASCII letter.
    BadSegmentStart {
        /// The character it starts with.
        found: char,
    },
    /// A segment holds something other than a lowercase ASCII letter, a digit or `_`.
    BadCharacter {
        /// The first such character.
        found: char,
    },
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("a command id cannot be empty"),
            Self::TooLong { len } => {
                write!(f, "a command id is at most {MAX_ID_LEN} bytes, not {len}")
            }
            Self::EmptySegment => f.write_str("a command id has an empty segment"),
            Self::BadSegmentStart { found } => write!(
                f,
                "a command id segment starts with a lowercase letter, not {found:?}"
            ),
            Self::BadCharacter { found } => write!(
                f,
                "a command id holds only lowercase letters, digits, '_' and '.', not {found:?}"
            ),
        }
    }
}

impl std::error::Error for IdError {}
