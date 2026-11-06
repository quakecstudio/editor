// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::io::Cursor;

use super::{BRIDGE_CONTENT_TYPE, BRIDGE_MAX_CONTENT, FrameError, read_frame, write_frame};

fn read(bytes: &[u8]) -> Result<Option<Vec<u8>>, FrameError> {
    read_frame(&mut Cursor::new(bytes.to_vec()))
}

// spec BRIDGE-1.2: a frame is header fields ended by \r\n, an empty line and Content-Length
// bytes; writing one and reading it gives the content back
#[test]
fn a_frame_round_trips() {
    let mut out = Vec::new();
    write_frame(&mut out, b"{\"a\":1}").unwrap();
    assert_eq!(out, b"Content-Length: 7\r\n\r\n{\"a\":1}");
    let mut reader = Cursor::new([out.clone(), out].concat());
    assert_eq!(read_frame(&mut reader).unwrap().unwrap(), b"{\"a\":1}");
    assert_eq!(read_frame(&mut reader).unwrap().unwrap(), b"{\"a\":1}");
    assert!(
        read_frame(&mut reader).unwrap().is_none(),
        "the stream ended between frames"
    );
}

// spec BRIDGE-1.2: Content-Type, when present, is the one encoding; any other is refused
#[test]
fn the_content_type_is_the_one_encoding() {
    let framed = format!("Content-Type: {BRIDGE_CONTENT_TYPE}\r\nContent-Length: 2\r\n\r\n{{}}");
    assert_eq!(read(framed.as_bytes()).unwrap().unwrap(), b"{}");
    let other = b"Content-Type: text/plain\r\nContent-Length: 2\r\n\r\n{}";
    assert!(matches!(read(other), Err(FrameError::ContentType(_))));
}

// spec BRIDGE-1.2: Content-Length is required and a number; the header is ASCII lines ended
// by \r\n; a stream that ends inside a frame, or a length past the limit, is refused
#[test]
fn a_bad_header_is_refused() {
    for bad in [
        &b"\r\n{}"[..],
        b"Content-Length: two\r\n\r\n{}",
        b"Content-Length 2\r\n\r\n{}",
        b"Content-Length: 2\n\n{}",
        b"Content-Length: 2\r\nX-\xe9: 1\r\n\r\n{}",
    ] {
        assert!(matches!(read(bad), Err(FrameError::Header(_))), "{bad:?}");
    }
    assert!(matches!(
        read(b"Content-Length: 9\r\n\r\n{}"),
        Err(FrameError::Io(_))
    ));
    assert!(matches!(
        read(b"Content-Length: 2\r\n"),
        Err(FrameError::Io(_))
    ));
    let huge = format!("Content-Length: {}\r\n\r\n", BRIDGE_MAX_CONTENT + 1);
    assert!(matches!(
        read(huge.as_bytes()),
        Err(FrameError::TooLarge(_))
    ));
}
