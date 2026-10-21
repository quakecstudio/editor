// SPDX-License-Identifier: GPL-3.0-or-later

//! The base protocol's framing: a header part, then a JSON-RPC content part.
//!
//! LSP 3.18, "Header Part": "Each header field is terminated by '\r\n'. Considering the last
//! header field and the overall header itself are each terminated with '\r\n', and that at
//! least one header is mandatory, this means that two '\r\n' sequences always immediately
//! precede the content part of a message." `Content-Length` is "The length of the content part
//! in bytes. This header is required." (`specs/lsp/3.18/specification.md`).

use std::fmt;
use std::io::{self, BufRead, Write};

/// Why a message could not be read.
#[derive(Debug)]
pub enum TransportError {
    /// Reading or writing failed.
    Io(io::Error),
    /// A header line is not `Name: value` in ASCII.
    BadHeader(String),
    /// The header part has no `Content-Length`, or its value is not a number.
    NoLength,
    /// The input ended inside a message.
    Truncated,
    /// A message is longer than [`MAX_CONTENT`].
    TooLarge(usize),
}

/// The longest content part read, 256 MiB: the base protocol sets no limit, and a length past
/// any real message would otherwise be allocated whole before a byte of it is read.
pub const MAX_CONTENT: usize = 1 << 28;

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::BadHeader(line) => write!(f, "bad header line {line:?}"),
            Self::NoLength => f.write_str("a message has no Content-Length"),
            Self::Truncated => f.write_str("the input ended inside a message"),
            Self::TooLarge(length) => {
                write!(
                    f,
                    "a message of {length} bytes is longer than {MAX_CONTENT}"
                )
            }
        }
    }
}

impl std::error::Error for TransportError {}

impl From<io::Error> for TransportError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Reads the next message's content part; `None` at the end of the input between messages.
///
/// # Errors
///
/// [`TransportError`] for a header the base protocol does not allow, or input that ends
/// inside a message.
pub fn read_message<R: BufRead>(input: &mut R) -> Result<Option<Vec<u8>>, TransportError> {
    let mut length = None;
    let mut first = true;
    loop {
        let mut line = Vec::new();
        let read = input.read_until(b'\n', &mut line)?;
        if read == 0 {
            return if first {
                Ok(None)
            } else {
                Err(TransportError::Truncated)
            };
        }
        first = false;
        let Some(line) = line.strip_suffix(b"\r\n") else {
            return Err(TransportError::BadHeader(
                String::from_utf8_lossy(&line).into_owned(),
            ));
        };
        if line.is_empty() {
            break;
        }
        // LSP 3.18: "The header part is encoded using the 'ascii' encoding."
        let text = std::str::from_utf8(line)
            .ok()
            .filter(|t| t.is_ascii())
            .ok_or_else(|| TransportError::BadHeader(String::from_utf8_lossy(line).into_owned()))?;
        let (name, value) = text
            .split_once(": ")
            .ok_or_else(|| TransportError::BadHeader(text.to_owned()))?;
        if name.eq_ignore_ascii_case("Content-Length") {
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| TransportError::NoLength)?,
            );
        }
    }
    let length = length.ok_or(TransportError::NoLength)?;
    if length > MAX_CONTENT {
        return Err(TransportError::TooLarge(length));
    }
    let mut content = vec![0; length];
    input.read_exact(&mut content).map_err(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            TransportError::Truncated
        } else {
            TransportError::Io(err)
        }
    })?;
    Ok(Some(content))
}

/// Writes one message with its header part, and flushes it.
///
/// # Errors
///
/// When writing fails.
pub fn write_message<W: Write>(output: &mut W, content: &[u8]) -> io::Result<()> {
    write!(output, "Content-Length: {}\r\n\r\n", content.len())?;
    output.write_all(content)?;
    output.flush()
}
