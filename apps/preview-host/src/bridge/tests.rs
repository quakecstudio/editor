// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{GenericFilePath, Stream, ToFsName as _};

use qs_bridge::{Answer, Capabilities, Host, HostInfo, Id, read_frame, write_frame};
use serde_json::{Value, json};

use super::Bridge;

struct Fake;

impl Host for Fake {
    fn info(&self) -> HostInfo {
        HostInfo {
            name: "fake".to_owned(),
            version: "0".to_owned(),
            engine: "test".to_owned(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::of(["pause"])
    }

    fn call(&mut self, _id: &Id, _method: &str, _params: &Value) -> Answer {
        Answer::Now(Ok(json!({})))
    }

    fn shutdown(&mut self) {}
}

/// The tests' sockets so far, for a name of each test's own.
static SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// A socket or named pipe name of this test's own (BRIDGE-1.1).
fn socket_path() -> PathBuf {
    let name = format!(
        "preview-host-{}-{}",
        std::process::id(),
        SOCKETS.fetch_add(1, Ordering::Relaxed)
    );
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    } else {
        std::env::temp_dir().join(format!("{name}.sock"))
    }
}

/// A bridge listening at a path of its own, and a client connected to it.
fn pair() -> (Bridge, Stream) {
    let path = socket_path();
    let at = path.clone();
    let host = std::thread::spawn(move || Bridge::listen(&at).unwrap());
    let client = loop {
        let name = path.as_os_str().to_fs_name::<GenericFilePath>().unwrap();
        if let Ok(stream) = Stream::connect(name) {
            break stream;
        }
        std::thread::yield_now();
    };
    (host.join().unwrap(), client)
}

// spec BRIDGE-1.1: the host serves one client; a second connection is closed at once
#[test]
fn a_second_client_is_turned_away() {
    let path = socket_path();
    let at = path.clone();
    let host = std::thread::spawn(move || Bridge::listen(&at).unwrap());
    let connect = || loop {
        let name = path.as_os_str().to_fs_name::<GenericFilePath>().unwrap();
        if let Ok(stream) = Stream::connect(name) {
            break stream;
        }
        std::thread::yield_now();
    };
    let first = connect();
    let _bridge = host.join().unwrap();
    let second = connect();
    assert!(
        read_frame(&mut BufReader::new(&second)).unwrap().is_none(),
        "closed"
    );
    drop(first);
}

fn send(client: &mut Stream, value: &Value) {
    write_frame(client, value.to_string().as_bytes()).unwrap();
}

fn receive(client: &Stream) -> Value {
    let content = read_frame(&mut BufReader::new(client)).unwrap().unwrap();
    serde_json::from_slice(&content).unwrap()
}

/// Answers the next thing the client sent.
fn settle(bridge: &mut Bridge) {
    bridge.handle_next(&mut Fake);
}

// spec BRIDGE-1.2, BRIDGE-2.2: a framed initialize is answered in a frame; events are sent
// only after it
#[test]
fn a_client_initializes_and_then_hears_events() {
    let (mut bridge, mut client) = pair();
    bridge.event("console/line", json!({ "text": "before", "level": "info" }));
    send(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                 "params": { "protocolVersion": "1.0", "client": { "name": "t" } } }),
    );
    settle(&mut bridge);
    let answer = receive(&client);
    assert_eq!(answer["id"], 1);
    assert_eq!(answer["result"]["capabilities"]["pause"], true);
    bridge.event("console/line", json!({ "text": "after", "level": "info" }));
    let line = receive(&client);
    assert_eq!(line["method"], "console/line");
    assert_eq!(
        line["params"]["text"], "after",
        "the event before initialize was not sent"
    );
}

// spec BRIDGE-1.4: content that is not JSON is answered with -32700 and a null id, and the
// session goes on
#[test]
fn an_unreadable_message_is_answered_and_the_session_goes_on() {
    let (mut bridge, mut client) = pair();
    write_frame(&mut client, b"{not json").unwrap();
    settle(&mut bridge);
    let answer = receive(&client);
    assert_eq!(answer["error"]["code"], -32700);
    assert_eq!(answer["id"], Value::Null);
    assert!(bridge.is_open());
}

// spec BRIDGE-1.1: the host's bridge closes when the client's connection closes
#[test]
fn the_bridge_closes_with_the_client() {
    let (mut bridge, client) = pair();
    drop(client);
    settle(&mut bridge);
    assert!(!bridge.is_open());
    assert!(!bridge.poll(&mut Fake));
}

// spec BRIDGE-2.4: initialize with another major closes the connection
#[test]
fn another_major_closes_the_bridge() {
    let (mut bridge, mut client) = pair();
    send(
        &mut client,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                 "params": { "protocolVersion": "9.0", "client": { "name": "t" } } }),
    );
    settle(&mut bridge);
    assert_eq!(receive(&client)["error"]["code"], -32001);
    assert!(!bridge.is_open());
}
