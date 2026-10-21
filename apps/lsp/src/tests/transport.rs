// SPDX-License-Identifier: GPL-3.0-or-later

//! The base protocol's framing.

use crate::transport::{MAX_CONTENT, TransportError, read_message, write_message};

/// The first message of `input`.
fn first(input: &[u8]) -> Result<Option<Vec<u8>>, TransportError> {
    let mut input = input;
    read_message(&mut input)
}

// LSP 3.18, "Header Part": "two '\r\n' sequences always immediately precede the content part of
// a message", `Content-Length` "is required", and `Content-Type` may be given too.
#[test]
fn a_message_and_its_headers() {
    assert_eq!(
        first(b"Content-Length: 2\r\n\r\n{}").unwrap(),
        Some(b"{}".to_vec())
    );
    assert_eq!(
        first(b"content-length: 2\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}")
            .unwrap(),
        Some(b"{}".to_vec())
    );
    let mut two: &[u8] = b"Content-Length: 1\r\n\r\naContent-Length: 1\r\n\r\nb";
    assert_eq!(read_message(&mut two).unwrap(), Some(b"a".to_vec()));
    assert_eq!(read_message(&mut two).unwrap(), Some(b"b".to_vec()));
    assert_eq!(read_message(&mut two).unwrap(), None);
}

// LSP 3.18, "Header Part": each header field is "a name and a value, separated by ': '" and
// "terminated by '\r\n'", in "the 'ascii' encoding".
#[test]
fn malformed_headers() {
    assert!(matches!(
        first(b"Content-Length: 2\n\n{}"),
        Err(TransportError::BadHeader(_))
    ));
    assert!(matches!(
        first(b"Content-Length=2\r\n\r\n{}"),
        Err(TransportError::BadHeader(_))
    ));
    assert!(matches!(
        first("Cont\u{e9}nt: 2\r\n\r\n".as_bytes()),
        Err(TransportError::BadHeader(_))
    ));
    assert!(matches!(
        first(b"Content-Type: x\r\n\r\n{}"),
        Err(TransportError::NoLength)
    ));
    assert!(matches!(
        first(b"Content-Length: two\r\n\r\n"),
        Err(TransportError::NoLength)
    ));
    assert!(matches!(
        first(b"Content-Length: 5\r\n\r\n{}"),
        Err(TransportError::Truncated)
    ));
    assert!(matches!(
        first(b"Content-Length: 5\r\n"),
        Err(TransportError::Truncated)
    ));
    let huge = format!("Content-Length: {}\r\n\r\n", MAX_CONTENT + 1);
    assert!(matches!(
        first(huge.as_bytes()),
        Err(TransportError::TooLarge(_))
    ));
}

// LSP 3.18, "Header Part": the server writes the length of the content part in bytes.
#[test]
fn writing_a_message() {
    let mut out = Vec::new();
    write_message(&mut out, "{\"a\":\"\u{e9}\"}".as_bytes()).unwrap();
    assert_eq!(
        out,
        "Content-Length: 10\r\n\r\n{\"a\":\"\u{e9}\"}".as_bytes()
    );
}

// Every error says what went wrong.
#[test]
fn errors_have_messages() {
    let io = TransportError::from(std::io::Error::other("gone"));
    assert_eq!(io.to_string(), "gone");
    assert_eq!(
        TransportError::BadHeader("x".to_owned()).to_string(),
        "bad header line \"x\""
    );
    assert_eq!(
        TransportError::NoLength.to_string(),
        "a message has no Content-Length"
    );
    assert_eq!(
        TransportError::Truncated.to_string(),
        "the input ended inside a message"
    );
    assert_eq!(
        TransportError::TooLarge(7).to_string(),
        format!("a message of 7 bytes is longer than {MAX_CONTENT}")
    );
}
