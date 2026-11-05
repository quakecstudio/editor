// SPDX-License-Identifier: GPL-3.0-or-later

//! BRIDGE-1.2: a message is a header part, `Name: value` fields each ended by `\r\n`, an empty
//! line, then `Content-Length` bytes of content, as LSP's base protocol frames them.

use std::io::{self, BufRead, Write};

/// The one content type a header may name (BRIDGE-1.2).
pub const BRIDGE_CONTENT_TYPE: &str = "application/vscode-jsonrpc; charset=utf-8";

/// The longest content a frame may carry, 64 MiB: a screenshot's PNG in base64 is far below
/// it, and a larger length is refused before anything is allocated.
pub const BRIDGE_MAX_CONTENT: usize = 64 * 1024 * 1024;

/// Why a frame could not be read; the connection is then closed (BRIDGE-1.2).
#[derive(Debug)]
pub enum FrameError {
    /// Reading failed, or the stream ended inside a frame.
    Io(io::Error),
    /// A header line is not `Name: value` in ASCII, or `Content-Length` is missing or no
    /// number.
    Header(String),
    /// A `Content-Type` other than [`BRIDGE_CONTENT_TYPE`].
    ContentType(String),
    /// A `Content-Length` past [`BRIDGE_MAX_CONTENT`].
    TooLarge(usize),
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "reading a frame: {error}"),
            Self::Header(line) => write!(f, "a bad header: {line:?}"),
            Self::ContentType(kind) => {
                write!(f, "content type {kind:?} is not {BRIDGE_CONTENT_TYPE:?}")
            }
            Self::TooLarge(length) => write!(f, "content of {length} bytes is over the limit"),
        }
    }
}

impl core::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The content of the next frame of `reader`, `None` when it ends before a header starts.
///
/// # Errors
///
/// [`FrameError`] for a frame that does not read.
pub fn read_frame<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, FrameError> {
    let mut length = None;
    let mut started = false;
    loop {
        let mut line = Vec::new();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            if started {
                return Err(FrameError::Io(io::ErrorKind::UnexpectedEof.into()));
            }
            return Ok(None);
        }
        started = true;
        let Some(line) = line.strip_suffix(b"\r\n") else {
            return Err(FrameError::Header(
                String::from_utf8_lossy(&line).into_owned(),
            ));
        };
        if line.is_empty() {
            break;
        }
        if !line.is_ascii() {
            return Err(FrameError::Header(
                String::from_utf8_lossy(line).into_owned(),
            ));
        }
        let text = String::from_utf8_lossy(line);
        let Some((name, value)) = text.split_once(':') else {
            return Err(FrameError::Header(text.into_owned()));
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("Content-Length") {
            let parsed: usize = value
                .parse()
                .map_err(|_| FrameError::Header(text.clone().into_owned()))?;
            length = Some(parsed);
        } else if name.eq_ignore_ascii_case("Content-Type") && value != BRIDGE_CONTENT_TYPE {
            return Err(FrameError::ContentType(value.to_owned()));
        }
    }
    let length = length.ok_or_else(|| FrameError::Header("no Content-Length".to_owned()))?;
    if length > BRIDGE_MAX_CONTENT {
        return Err(FrameError::TooLarge(length));
    }
    let mut content = vec![0; length];
    reader.read_exact(&mut content)?;
    Ok(Some(content))
}

/// Writes `content` to `writer` as one frame, with its `Content-Length`.
///
/// # Errors
///
/// Whatever writing fails with.
pub fn write_frame<W: Write>(writer: &mut W, content: &[u8]) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", content.len())?;
    writer.write_all(content)?;
    writer.flush()
}

#[cfg(test)]
mod tests;
