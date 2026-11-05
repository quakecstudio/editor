// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, index or match is a failed test"
)]

use serde_json::json;

use super::{ErrorCode, Id, Message, RpcError};

fn error_of(content: &str) -> (Option<Id>, ErrorCode) {
    match *Message::parse(content.as_bytes()).unwrap_err() {
        Message::Response {
            id,
            outcome: Err(error),
        } => (id, error.code),
        other => panic!("not an error response: {other:?}"),
    }
}

// spec BRIDGE-1.3: a request has a method and an id, a notification a method and no id, a
// response an id and a result or an error; params default to {}
#[test]
fn the_three_kinds_parse() {
    let request = Message::parse(br#"{"jsonrpc":"2.0","id":1,"method":"pause"}"#).unwrap();
    assert_eq!(
        request,
        Message::Request {
            id: Id::Number(1),
            method: "pause".to_owned(),
            params: json!({})
        }
    );
    let note = Message::parse(br#"{"jsonrpc":"2.0","method":"x","params":{"a":1}}"#).unwrap();
    assert!(matches!(note, Message::Notification { ref params, .. } if params["a"] == 1));
    let response = Message::parse(br#"{"jsonrpc":"2.0","id":"s","result":null}"#).unwrap();
    assert!(matches!(
        response,
        Message::Response {
            id: Some(Id::Text(_)),
            outcome: Ok(_)
        }
    ));
    let failed =
        Message::parse(br#"{"jsonrpc":"2.0","id":-1,"error":{"code":-32004,"message":"m"}}"#)
            .unwrap();
    assert!(matches!(
        failed,
        Message::Response {
            outcome: Err(RpcError {
                code: ErrorCode::NoSuchEdict,
                ..
            }),
            ..
        }
    ));
}

// spec BRIDGE-1.3: a batch is answered with -32600 and no message is read from it
// spec BRIDGE-1.4: content that is not JSON is -32700, an envelope that is not JSON-RPC 2.0
// is -32600, and the id is null when the request's could not be read
#[test]
fn bad_messages_are_answered_with_their_errors() {
    assert_eq!(error_of("{"), (None, ErrorCode::ParseError));
    assert_eq!(error_of("[{}]"), (None, ErrorCode::InvalidRequest));
    assert_eq!(
        error_of(r#"{"jsonrpc":"1.0","id":3,"method":"pause"}"#),
        (Some(Id::Number(3)), ErrorCode::InvalidRequest)
    );
    assert_eq!(
        error_of(r#"{"jsonrpc":"2.0","id":[1],"method":"pause"}"#),
        (None, ErrorCode::InvalidRequest)
    );
    assert_eq!(
        error_of(r#"{"jsonrpc":"2.0","id":1,"method":"pause","result":1}"#).1,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        error_of(r#"{"jsonrpc":"2.0","id":1,"method":"pause","params":[1]}"#).1,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        error_of(r#"{"jsonrpc":"2.0","id":1,"result":1,"error":{"code":1,"message":""}}"#).1,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        error_of(r#"{"jsonrpc":"2.0"}"#).1,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        error_of(r#"{"jsonrpc":"2.0","method":7}"#).1,
        ErrorCode::InvalidRequest
    );
}

// spec BRIDGE-7.1: each code has its number
#[test]
fn the_codes_are_the_specs() {
    for (code, number) in [
        (ErrorCode::ParseError, -32700),
        (ErrorCode::InvalidRequest, -32600),
        (ErrorCode::MethodNotFound, -32601),
        (ErrorCode::InvalidParams, -32602),
        (ErrorCode::InternalError, -32603),
        (ErrorCode::IncompatibleVersion, -32001),
        (ErrorCode::NotPaused, -32002),
        (ErrorCode::NotInitialized, -32003),
        (ErrorCode::NoSuchEdict, -32004),
        (ErrorCode::Unsupported, -32005),
        (ErrorCode::BadValue, -32006),
        (ErrorCode::NoSuchBreakpoint, -32007),
    ] {
        assert_eq!(code.code(), number, "{code:?}");
    }
}

// spec BRIDGE-1.4: a message written and read again is the same message
#[test]
fn messages_round_trip() {
    let error = RpcError {
        code: ErrorCode::BadValue,
        message: "no".to_owned(),
        data: Some(json!({ "field": "health" })),
    };
    for message in [
        Message::Request {
            id: Id::Text("a".to_owned()),
            method: "step".to_owned(),
            params: json!({ "kind": "over" }),
        },
        Message::Notification {
            method: "console/line".to_owned(),
            params: json!({ "text": "hi" }),
        },
        Message::Response {
            id: Some(Id::Number(2)),
            outcome: Ok(json!(true)),
        },
        Message::Response {
            id: Some(Id::Number(2)),
            outcome: Err(error),
        },
    ] {
        assert_eq!(Message::parse(&message.to_bytes()).unwrap(), message);
    }
    let null = Message::Response {
        id: None,
        outcome: Err(RpcError::new(ErrorCode::ParseError, "x")),
    };
    assert_eq!(null.to_json()["id"], json!(null));
}
