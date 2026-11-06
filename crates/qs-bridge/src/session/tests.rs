// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "test code: a failed unwrap or an unexpected reply is a failed test"
)]

use serde_json::{Value, json};

use super::{
    Answer, BRIDGE_COMMANDS, Capabilities, Host, HostInfo, Reply, Session, event, is_flagged,
    schema_checks,
};
use crate::message::{ErrorCode, Id, Message};

/// A host that accepts `pause` and `step`, and records what it is asked.
#[derive(Default)]
struct Fake {
    calls: Vec<String>,
    stopped: bool,
}

impl Host for Fake {
    fn info(&self) -> HostInfo {
        HostInfo {
            name: "fake".to_owned(),
            version: "1".to_owned(),
            engine: "test".to_owned(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::of(["pause", "step", "screenshot", "initialize"])
    }

    fn call(&mut self, _id: &Id, method: &str, _params: &Value) -> Answer {
        self.calls.push(method.to_owned());
        if method == "screenshot" {
            return Answer::Later;
        }
        Answer::Now(Ok(json!({})))
    }

    fn shutdown(&mut self) {
        self.stopped = true;
    }
}

fn request(method: &str, params: Value) -> Message {
    Message::Request {
        id: Id::Number(1),
        method: method.to_owned(),
        params,
    }
}

fn outcome(reply: Reply) -> Result<Value, ErrorCode> {
    match reply.response {
        Some(Message::Response { outcome, .. }) => outcome.map_err(|e| e.code),
        other => panic!("no response: {other:?}"),
    }
}

fn initialized(host: &mut Fake) -> Session {
    let mut session = Session::new();
    let params = json!({ "protocolVersion": "1.0", "client": { "name": "test" } });
    assert!(outcome(session.handle(host, request("initialize", params))).is_ok());
    session
}

// spec BRIDGE-2.1: a request before initialize is -32003, and a notification before it is
// ignored
#[test]
fn initialize_comes_first() {
    let mut host = Fake::default();
    let mut session = Session::new();
    assert!(!session.is_initialized());
    assert_eq!(
        outcome(session.handle(&mut host, request("pause", json!({})))),
        Err(ErrorCode::NotInitialized)
    );
    let note = Message::Notification {
        method: "pause".to_owned(),
        params: json!({}),
    };
    assert_eq!(session.handle(&mut host, note).response, None);
    assert!(host.calls.is_empty());
}

// spec BRIDGE-2.2: initialize is answered with the version, the host and its capabilities,
// one boolean per flagged command, as the result's schema says
// spec BRIDGE-2.6: the capabilities are the flagged commands, initialize, shutdown and
// inspect among none of them
#[test]
fn initialize_answers_with_the_host_and_its_capabilities() {
    let mut host = Fake::default();
    let mut session = Session::new();
    let params = json!({ "protocolVersion": "1.3", "client": { "name": "test" } });
    let result = outcome(session.handle(&mut host, request("initialize", params))).unwrap();
    assert!(session.is_initialized());
    assert_eq!(result["protocolVersion"], "1.1");
    assert_eq!(result["host"]["engine"], "test");
    assert_eq!(result["capabilities"]["pause"], true);
    assert_eq!(result["capabilities"]["reload"], false);
    assert_eq!(result["capabilities"].as_object().unwrap().len(), 16);
    assert!(result["capabilities"].get("inspect").is_none());
    assert!(schema_checks("initialize.result", &result));
    assert_eq!(BRIDGE_COMMANDS.iter().filter(|(_, f)| *f).count(), 16);
    assert!(
        !is_flagged("initialize")
            && !is_flagged("inspect")
            && is_flagged("evaluate")
            && is_flagged("window/place")
    );
}

// spec BRIDGE-2.7: another major is -32001 and the connection closes; a higher minor is
// accepted; params that fail initialize's schema are -32602
#[test]
fn the_major_version_must_match() {
    let mut host = Fake::default();
    let mut session = Session::new();
    let params = json!({ "protocolVersion": "2.0", "client": { "name": "test" } });
    let reply = session.handle(&mut host, request("initialize", params));
    assert!(reply.close);
    assert_eq!(outcome(reply), Err(ErrorCode::IncompatibleVersion));
    assert!(!session.is_initialized());
    let bad = json!({ "protocolVersion": "one", "client": { "name": "test" } });
    assert_eq!(
        outcome(session.handle(&mut host, request("initialize", bad))),
        Err(ErrorCode::InvalidParams)
    );
}

// spec BRIDGE-2.6: a command whose capability is off is -32005
// spec BRIDGE-4.2: params that fail the command's schema are -32602 and nothing runs
// spec BRIDGE-6.2: an unknown method is -32601
#[test]
fn commands_are_checked_before_they_run() {
    let mut host = Fake::default();
    let mut session = initialized(&mut host);
    assert_eq!(
        outcome(session.handle(&mut host, request("reload", json!({})))),
        Err(ErrorCode::Unsupported)
    );
    assert_eq!(
        outcome(session.handle(&mut host, request("step", json!({ "kind": "sideways" })))),
        Err(ErrorCode::InvalidParams)
    );
    assert_eq!(
        outcome(session.handle(&mut host, request("fly", json!({})))),
        Err(ErrorCode::MethodNotFound)
    );
    assert!(host.calls.is_empty());
    assert!(outcome(session.handle(&mut host, request("step", json!({ "kind": "over" })))).is_ok());
    assert_eq!(host.calls, ["step"]);
}

// spec BRIDGE-4.9: a command the host answers later, as screenshot waits for a frame, gets
// no response from the session; the host sends it
#[test]
fn a_command_answered_later_has_no_response_yet() {
    let mut host = Fake::default();
    let mut session = initialized(&mut host);
    let reply = session.handle(&mut host, request("screenshot", json!({})));
    assert_eq!(reply.response, None);
    assert!(!reply.close);
    assert_eq!(host.calls, ["screenshot"]);
}

// spec BRIDGE-2.5: shutdown stops the game and is answered with null
#[test]
fn shutdown_stops_the_game() {
    let mut host = Fake::default();
    let mut session = initialized(&mut host);
    assert_eq!(
        outcome(session.handle(&mut host, request("shutdown", json!({})))),
        Ok(Value::Null)
    );
    assert!(host.stopped);
}

// spec BRIDGE-3.1: an event is a notification whose parameters its schema checks
#[test]
fn events_are_notifications() {
    let loaded = event(
        "map/loaded",
        json!({ "time": 1.0, "map": "e1m1", "entities": 3 }),
    );
    assert!(matches!(loaded, Message::Notification { ref method, .. } if method == "map/loaded"));
    let Message::Notification { params, .. } = loaded else {
        panic!("not a notification");
    };
    assert!(schema_checks("event.map.loaded", &params));
    assert!(!schema_checks("event.map.loaded", &json!({ "map": 7 })));
}
