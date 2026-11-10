// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{GenericFilePath, Stream, ToFsName as _};
use serde_json::json;

use super::{SessionSpec, session};
use crate::remote::{Client, Host, RemoteError};

/// A socket name of test `name`'s own.
fn socket(name: &str) -> PathBuf {
    let word = format!("qs-commands-{name}-{}", std::process::id());
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{word}"))
    } else {
        std::env::temp_dir().join(word).join("app.sock")
    }
}

// No spec statement: AGT-7, a client lists the host's commands and runs them in the host's
// session, which the host's thread changes; a command's failure is the command's answer, not
// the connection's, and each request wakes the host
#[test]
fn a_client_runs_commands_in_the_hosts_session() {
    let path = socket("runs");
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&woken);
    let host = Host::listen(&path, move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    let client_path = path.clone();
    let client = std::thread::spawn(move || {
        let mut client = Client::connect(&client_path).unwrap();
        let commands = client.commands().unwrap();
        let appended = client
            .execute("doc.append", json!({ "text": "def" }))
            .unwrap();
        let failed = client.execute("doc.fail", json!({})).unwrap();
        let unknown = client.execute("doc.nothing", json!({})).unwrap();
        (commands, appended, failed, unknown)
    });
    let mut session = session(SessionSpec { text: "abc" });
    for _ in 0..4 {
        assert!(host.answer_next(&mut session));
    }
    let (commands, appended, failed, unknown) = client.join().unwrap();
    let append = commands.iter().find(|c| c.id() == "doc.append").unwrap();
    assert_eq!(append.summary(), "Appends text.");
    assert_eq!(append.params()["type"], "object");
    assert_eq!(appended, Ok(json!(6)));
    assert_eq!(
        session.context().text,
        "abcdef",
        "the host's session changed"
    );
    assert_eq!(
        failed,
        Err("doc.fail failed: the test command refused".to_owned())
    );
    assert!(unknown.unwrap_err().contains("doc.nothing"));
    assert_eq!(woken.load(Ordering::SeqCst), 4);
    assert_eq!(host.answer(&mut session), 0, "nothing more waits");
}

// No spec statement: AGT-7, a line that is no request is answered with an error and the
// connection carries on; nothing reaches the host's session
#[test]
fn a_bad_line_is_answered_with_an_error() {
    let path = socket("bad");
    let host = Host::listen(&path, || {}).unwrap();
    let raw = path.clone();
    let client = std::thread::spawn(move || {
        let name = raw.as_os_str().to_fs_name::<GenericFilePath>().unwrap();
        let mut reader = BufReader::new(Stream::connect(name).unwrap());
        reader.get_mut().write_all(b"{\"nope\": 1}\n").unwrap();
        let mut bad = String::new();
        reader.read_line(&mut bad).unwrap();
        reader.get_mut().write_all(b"\"commands\"\n").unwrap();
        let mut listed = String::new();
        reader.read_line(&mut listed).unwrap();
        (bad, listed)
    });
    let mut session = session(SessionSpec::default());
    assert!(host.answer_next(&mut session));
    let (bad, listed) = client.join().unwrap();
    assert!(
        bad.contains("\"error\"") && bad.contains("not a request"),
        "{bad}"
    );
    assert!(listed.contains("doc.append"), "{listed}");
}

// No spec statement: AGT-7, a second host on a live socket is refused, a client with no host
// gets an I/O error, and a socket file left by a host that has gone is taken over
#[test]
fn one_host_answers_on_a_socket() {
    let path = socket("one");
    let first = Host::listen(&path, || {}).unwrap();
    assert!(matches!(
        Host::listen(&path, || {}),
        Err(RemoteError::InUse)
    ));
    drop(first);
    let nobody = socket("nobody");
    assert!(matches!(Client::connect(&nobody), Err(RemoteError::Io(_))));
    if !cfg!(windows) {
        let stale = socket("stale");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, b"").unwrap();
        let host = Host::listen(&stale, || {}).unwrap();
        let client = std::thread::spawn(move || Client::connect(&stale).unwrap().commands());
        let mut session = session(SessionSpec::default());
        assert!(host.answer_next(&mut session));
        assert!(!client.join().unwrap().unwrap().is_empty());
    }
    assert_eq!(
        RemoteError::InUse.to_string(),
        "another QuakeC Studio already answers on its socket"
    );
}
